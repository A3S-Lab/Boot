use std::time::Duration;

use a3s_orm::{
    sql_query, DecodeError, Executor, FromRow, FromValue, MigrationReport, Migrator,
    PostgresDialect, PostgresError, PostgresExecutor, PostgresRow, PostgresTransaction,
    PostgresTransactionError, Query, Row, SqlQuery,
};
use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use crate::{BootError, Result};

use super::super::{
    QueueJobFailure, QueueJobInfo, QueueJobOptions, QueueJobReceipt, QueueJobRetention,
    QueueJobState, QueueStats,
};
use super::deduplication::{
    find_deduplication_owner, lock_deduplication, release_expired_deduplication,
    replace_delayed_owner, store_successor, update_deduplication_expiry,
};
use super::lifecycle::{
    apply_retention, successor_from_active, successor_from_expired, terminal_completion,
    terminal_failure, ActiveJobRow, ExpiredJobRow,
};
use super::migrations::postgres_queue_migrations;

const STALLED_FAILURE: &str = "queue job exceeded its stalled lease recovery limit";

#[derive(Clone)]
pub(super) struct PostgresQueueStore {
    executor: PostgresExecutor,
    queue_name: String,
}

#[derive(Debug)]
pub(super) struct ClaimedJob {
    pub id: String,
    pub name: String,
    pub payload: Value,
    pub options: QueueJobOptions,
    pub lock_token: String,
}

struct ClaimedJobRow {
    id: String,
    name: String,
    payload_json: String,
    options_json: String,
    lock_token: String,
}

impl FromRow for ClaimedJobRow {
    fn from_row(row: &impl Row) -> std::result::Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            name: decode(row, 1)?,
            payload_json: decode(row, 2)?,
            options_json: decode(row, 3)?,
            lock_token: decode(row, 4)?,
        })
    }
}

struct JobInfoRow {
    id: String,
    name: String,
    state: String,
    payload_json: String,
}

impl FromRow for JobInfoRow {
    fn from_row(row: &impl Row) -> std::result::Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            name: decode(row, 1)?,
            state: decode(row, 2)?,
            payload_json: decode(row, 3)?,
        })
    }
}

struct FailureRow {
    id: String,
    name: String,
    message: Option<String>,
}

impl FromRow for FailureRow {
    fn from_row(row: &impl Row) -> std::result::Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            name: decode(row, 1)?,
            message: decode(row, 2)?,
        })
    }
}

impl PostgresQueueStore {
    pub(super) async fn connect(database_url: &str, queue_name: &str) -> Result<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url, 5).map_err(|error| {
            BootError::Internal(format!(
                "could not configure PostgreSQL Boot queue: {error}"
            ))
        })?;
        Self::from_executor(executor, queue_name).await
    }

    pub(super) async fn connect_verified(database_url: &str, queue_name: &str) -> Result<Self> {
        let executor = PostgresExecutor::connect_no_tls(database_url, 5).map_err(|error| {
            BootError::Internal(format!(
                "could not configure PostgreSQL Boot queue: {error}"
            ))
        })?;
        Self::from_executor_verified(executor, queue_name).await
    }

    pub(super) async fn from_executor(
        executor: PostgresExecutor,
        queue_name: &str,
    ) -> Result<Self> {
        let queue_name = validated_queue_name(queue_name)?;
        migrate_schema(&executor).await?;
        Ok(Self::new(executor, queue_name))
    }

    pub(super) async fn from_executor_verified(
        executor: PostgresExecutor,
        queue_name: &str,
    ) -> Result<Self> {
        let queue_name = validated_queue_name(queue_name)?;
        verify_schema(&executor).await?;
        Ok(Self::new(executor, queue_name))
    }

    fn new(executor: PostgresExecutor, queue_name: String) -> Self {
        Self {
            executor,
            queue_name,
        }
    }

    pub(super) fn queue_name(&self) -> &str {
        &self.queue_name
    }

    pub(super) async fn enqueue(
        &self,
        name: String,
        payload: Value,
        options: QueueJobOptions,
    ) -> Result<QueueJobReceipt> {
        let name = validate_job(name)?;
        validate_options(&options)?;
        let payload_json = serde_json::to_string(&payload).map_err(json_error)?;
        let options_json = serde_json::to_string(&options).map_err(json_error)?;
        let requested_job_id = options
            .job_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let queue_name = self.queue_name.clone();
        let result = self
            .executor
            .transaction(|transaction| {
                let name = name.clone();
                let payload_json = payload_json.clone();
                let options_json = options_json.clone();
                let requested_job_id = requested_job_id.clone();
                let options = options.clone();
                let queue_name = queue_name.clone();
                Box::pin(async move {
                    let now = now_nanos();
                    if let Some(deduplication) = options.deduplication.as_ref() {
                        lock_deduplication(transaction, &queue_name, &deduplication.id).await?;
                        release_expired_deduplication(
                            transaction,
                            &queue_name,
                            &deduplication.id,
                            now,
                        )
                        .await?;
                        if let Some((owner_id, owner_name, owner_state, available_at)) =
                            find_deduplication_owner(transaction, &queue_name, &deduplication.id)
                                .await?
                        {
                            if deduplication.extend {
                                update_deduplication_expiry(
                                    transaction,
                                    &queue_name,
                                    &owner_id,
                                    deduplication.ttl.map(|ttl| add_duration(now, ttl)),
                                    now,
                                )
                                .await?;
                            }
                            if owner_state == "active" && deduplication.keep_last_if_active {
                                store_successor(
                                    transaction,
                                    &queue_name,
                                    &owner_id,
                                    &requested_job_id,
                                    &name,
                                    &payload_json,
                                    &options_json,
                                    now,
                                )
                                .await?;
                            } else if owner_state == "pending"
                                && available_at > now
                                && deduplication.replace
                            {
                                replace_delayed_owner(
                                    transaction,
                                    &queue_name,
                                    &owner_id,
                                    &name,
                                    &payload_json,
                                    &options_json,
                                    &options,
                                    now,
                                )
                                .await?;
                            }
                            return Ok(QueueJobReceipt {
                                id: owner_id,
                                name: owner_name,
                            });
                        }
                    }

                    let inserted = insert_job(
                        transaction,
                        &queue_name,
                        &requested_job_id,
                        &name,
                        &payload_json,
                        &options_json,
                        &options,
                        now,
                    )
                    .await?;
                    if inserted {
                        return Ok(QueueJobReceipt {
                            id: requested_job_id,
                            name,
                        });
                    }
                    ensure_idempotent_job(
                        transaction,
                        &queue_name,
                        &requested_job_id,
                        &name,
                        &payload_json,
                        &options_json,
                    )
                    .await?;
                    Ok(QueueJobReceipt {
                        id: requested_job_id,
                        name,
                    })
                })
            })
            .await;
        map_transaction(result)
    }

    pub(super) async fn recover_expired(&self, limit: usize) -> Result<usize> {
        let queue_name = self.queue_name.clone();
        let limit = i64::try_from(limit).map_err(|error| {
            BootError::Internal(format!(
                "PostgreSQL queue recovery limit is invalid: {error}"
            ))
        })?;
        let result = self
            .executor
            .transaction(|transaction| {
                let queue_name = queue_name.clone();
                Box::pin(async move {
                    let now = now_nanos();
                    let expired = fetch_all_query(
                        transaction,
                        sql_query::<ExpiredJobRow>(
                            "SELECT job_id, lock_token, options_json, stalled_count, \
                             successor_job_id, successor_job_name, successor_payload_json, \
                             successor_options_json FROM boot_queue_jobs WHERE queue_name = ",
                        )
                        .bind(queue_name.clone())
                        .append(" AND state = 'active' AND lease_expires_at_nanos <= ")
                        .bind(now)
                        .append(" ORDER BY lease_expires_at_nanos ASC, job_id ASC FOR UPDATE SKIP LOCKED LIMIT ")
                        .bind(limit),
                    )
                    .await?;
                    for job in &expired {
                        let options: QueueJobOptions =
                            serde_json::from_str(&job.options_json).map_err(stored_json_error)?;
                        if job.stalled_count >= options.max_stalled_count {
                            terminal_failure(
                                transaction,
                                &queue_name,
                                &job.id,
                                &job.lock_token,
                                &options,
                                STALLED_FAILURE,
                                successor_from_expired(job),
                                now,
                            )
                            .await?;
                            apply_retention(
                                transaction,
                                &queue_name,
                                "failed",
                                &options,
                                now,
                            )
                            .await?;
                        } else {
                            execute_query(
                                transaction,
                                sql_query::<()>(
                                    "UPDATE boot_queue_jobs SET state = 'pending', worker_id = NULL, \
                                     lock_token = NULL, lease_expires_at_nanos = NULL, \
                                     attempts_made = GREATEST(attempts_made - 1, 0), \
                                     stalled_count = stalled_count + 1, updated_at_nanos = ",
                                )
                                .bind(now)
                                .append(" WHERE queue_name = ")
                                .bind(queue_name.clone())
                                .append(" AND job_id = ")
                                .bind(job.id.clone())
                                .append(" AND state = 'active' AND lock_token = ")
                                .bind(job.lock_token.clone()),
                            )
                            .await?;
                        }
                    }
                    Ok(expired.len())
                })
            })
            .await;
        map_transaction(result)
    }

    pub(super) async fn claim(
        &self,
        worker_id: &str,
        lease_duration: Duration,
    ) -> Result<Option<ClaimedJob>> {
        let now = now_nanos();
        let lock_token = Uuid::new_v4().to_string();
        let lease_expires_at = add_duration(now, lease_duration);
        let row = fetch_optional_query(
            &self.executor,
            sql_query::<ClaimedJobRow>(
                "WITH next_job AS (SELECT job_id FROM boot_queue_jobs WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(" AND state = 'pending' AND available_at_nanos <= ")
            .bind(now)
            .append(
                " ORDER BY priority ASC, \
                 CASE WHEN lifo THEN created_at_nanos END DESC NULLS LAST, \
                 CASE WHEN NOT lifo THEN created_at_nanos END ASC NULLS LAST, job_id ASC \
                 FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE boot_queue_jobs SET state = 'active', \
                 attempts_made = attempts_made + 1, worker_id = ",
            )
            .bind(worker_id)
            .append(", lock_token = ")
            .bind(lock_token)
            .append(", lease_expires_at_nanos = ")
            .bind(lease_expires_at)
            .append(", updated_at_nanos = ")
            .bind(now)
            .append(" FROM next_job WHERE boot_queue_jobs.queue_name = ")
            .bind(self.queue_name.clone())
            .append(
                " AND boot_queue_jobs.job_id = next_job.job_id RETURNING \
                 boot_queue_jobs.job_id, boot_queue_jobs.job_name, \
                 boot_queue_jobs.payload_json, boot_queue_jobs.options_json, \
                 boot_queue_jobs.lock_token",
            ),
        )
        .await?;
        row.map(claimed_job_from_row).transpose()
    }

    pub(super) async fn heartbeat(
        &self,
        job_id: &str,
        lock_token: &str,
        lease_duration: Duration,
    ) -> Result<()> {
        let now = now_nanos();
        let rows = execute_query(
            &self.executor,
            sql_query::<()>("UPDATE boot_queue_jobs SET lease_expires_at_nanos = ")
                .bind(add_duration(now, lease_duration))
                .append(", updated_at_nanos = ")
                .bind(now)
                .append(" WHERE queue_name = ")
                .bind(self.queue_name.clone())
                .append(" AND job_id = ")
                .bind(job_id)
                .append(" AND state = 'active' AND lock_token = ")
                .bind(lock_token),
        )
        .await?;
        require_fenced_row(rows, job_id, "heartbeat")
    }

    pub(super) async fn release(&self, job_id: &str, lock_token: &str) -> Result<()> {
        let rows = execute_query(
            &self.executor,
            sql_query::<()>(
                "UPDATE boot_queue_jobs SET state = 'pending', worker_id = NULL, \
                 lock_token = NULL, lease_expires_at_nanos = NULL, \
                 attempts_made = GREATEST(attempts_made - 1, 0), updated_at_nanos = ",
            )
            .bind(now_nanos())
            .append(" WHERE queue_name = ")
            .bind(self.queue_name.clone())
            .append(" AND job_id = ")
            .bind(job_id)
            .append(" AND state = 'active' AND lock_token = ")
            .bind(lock_token),
        )
        .await?;
        require_fenced_row(rows, job_id, "release")
    }

    pub(super) async fn complete(&self, job_id: &str, lock_token: &str) -> Result<()> {
        self.finish(job_id, lock_token, None).await
    }

    pub(super) async fn fail(&self, job_id: &str, lock_token: &str, message: String) -> Result<()> {
        self.finish(job_id, lock_token, Some(message)).await
    }

    async fn finish(&self, job_id: &str, lock_token: &str, failure: Option<String>) -> Result<()> {
        let queue_name = self.queue_name.clone();
        let job_id = job_id.to_string();
        let lock_token = lock_token.to_string();
        let result = self
            .executor
            .transaction(|transaction| {
                let queue_name = queue_name.clone();
                let job_id = job_id.clone();
                let lock_token = lock_token.clone();
                let failure = failure.clone();
                Box::pin(async move {
                    let row = fetch_optional_query(
                        transaction,
                        sql_query::<ActiveJobRow>(
                            "SELECT options_json, attempts_made, successor_job_id, \
                             successor_job_name, successor_payload_json, successor_options_json \
                             FROM boot_queue_jobs WHERE queue_name = ",
                        )
                        .bind(queue_name.clone())
                        .append(" AND job_id = ")
                        .bind(job_id.clone())
                        .append(" AND state = 'active' AND lock_token = ")
                        .bind(lock_token.clone())
                        .append(" FOR UPDATE"),
                    )
                    .await?
                    .ok_or_else(|| lease_conflict(&job_id, "finish"))?;
                    let options: QueueJobOptions =
                        serde_json::from_str(&row.options_json).map_err(stored_json_error)?;
                    let now = now_nanos();
                    if let Some(message) = failure {
                        if options.retry_policy.max_retries > 0
                            && row.attempts_made <= options.retry_policy.max_retries
                        {
                            let delay = options.retry_policy.delay_for_attempt(row.attempts_made);
                            let rows = execute_query(
                                transaction,
                                sql_query::<()>(
                                    "UPDATE boot_queue_jobs SET state = 'pending', \
                                     available_at_nanos = ",
                                )
                                .bind(add_duration(now, delay))
                                .append(
                                    ", worker_id = NULL, lock_token = NULL, \
                                         lease_expires_at_nanos = NULL, failed_reason = ",
                                )
                                .bind(message)
                                .append(", updated_at_nanos = ")
                                .bind(now)
                                .append(" WHERE queue_name = ")
                                .bind(queue_name)
                                .append(" AND job_id = ")
                                .bind(job_id.clone())
                                .append(" AND state = 'active' AND lock_token = ")
                                .bind(lock_token),
                            )
                            .await?;
                            return require_fenced_row(rows, &job_id, "retry");
                        }
                        terminal_failure(
                            transaction,
                            &queue_name,
                            &job_id,
                            &lock_token,
                            &options,
                            &message,
                            successor_from_active(&row),
                            now,
                        )
                        .await?;
                        apply_retention(transaction, &queue_name, "failed", &options, now).await?;
                    } else {
                        terminal_completion(
                            transaction,
                            &queue_name,
                            &job_id,
                            &lock_token,
                            &options,
                            successor_from_active(&row),
                            now,
                        )
                        .await?;
                        apply_retention(transaction, &queue_name, "completed", &options, now)
                            .await?;
                    }
                    Ok(())
                })
            })
            .await;
        map_transaction(result)
    }

    pub(super) async fn jobs(&self) -> Result<Vec<QueueJobInfo>> {
        fetch_all_query(
            &self.executor,
            sql_query::<JobInfoRow>(
                "SELECT job_id, job_name, state, payload_json FROM boot_queue_jobs \
                 WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(" ORDER BY created_at_nanos ASC, job_id ASC"),
        )
        .await?
        .into_iter()
        .map(job_info_from_row)
        .collect()
    }

    pub(super) async fn failures(&self) -> Result<Vec<QueueJobFailure>> {
        Ok(fetch_all_query(
            &self.executor,
            sql_query::<FailureRow>(
                "SELECT job_id, job_name, failed_reason FROM boot_queue_jobs \
                 WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(" AND state = 'failed' ORDER BY finished_at_nanos ASC, job_id ASC"),
        )
        .await?
        .into_iter()
        .map(|row| QueueJobFailure {
            id: row.id,
            name: row.name,
            message: row
                .message
                .unwrap_or_else(|| "job failed without a retained reason".to_string()),
        })
        .collect())
    }

    pub(super) async fn stats(&self) -> Result<QueueStats> {
        let rows = fetch_all_query(
            &self.executor,
            sql_query::<(String, i64)>(
                "SELECT state, COUNT(*)::BIGINT FROM boot_queue_jobs WHERE queue_name = ",
            )
            .bind(self.queue_name.clone())
            .append(" GROUP BY state"),
        )
        .await?;
        let mut stats = QueueStats::default();
        for (state, count) in rows {
            let count = usize::try_from(count).map_err(|error| {
                BootError::Internal(format!("stored PostgreSQL queue count is invalid: {error}"))
            })?;
            match state.as_str() {
                "pending" => stats.pending = count,
                "active" => stats.active = count,
                "completed" => stats.completed = count,
                "failed" => stats.failed = count,
                other => {
                    return Err(BootError::Internal(format!(
                        "stored PostgreSQL queue state is invalid: {other}"
                    )))
                }
            }
        }
        Ok(stats)
    }

    pub(super) async fn clear(&self) -> Result<()> {
        execute_query(
            &self.executor,
            sql_query::<()>("DELETE FROM boot_queue_jobs WHERE queue_name = ")
                .bind(self.queue_name.clone()),
        )
        .await?;
        Ok(())
    }
}

fn validated_queue_name(queue_name: &str) -> Result<String> {
    let queue_name = queue_name.trim();
    if queue_name.is_empty() {
        return Err(BootError::BadRequest(
            "PostgreSQL queue name cannot be empty".to_string(),
        ));
    }
    Ok(queue_name.to_string())
}

pub(super) async fn migrate_schema(executor: &PostgresExecutor) -> Result<MigrationReport> {
    Migrator::new(executor.clone())
        .run(postgres_queue_migrations())
        .await
        .map_err(|error| {
            BootError::Internal(format!("PostgreSQL Boot queue migration failed: {error}"))
        })
}

async fn verify_schema(executor: &PostgresExecutor) -> Result<()> {
    Migrator::new(executor.clone())
        .verify_required(postgres_queue_migrations())
        .await
        .map_err(|error| {
            BootError::Internal(format!(
                "PostgreSQL Boot queue schema admission failed: {error}; run the Boot migrator before serving"
            ))
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_job(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    name: &str,
    payload_json: &str,
    options_json: &str,
    options: &QueueJobOptions,
    now: i64,
) -> Result<bool> {
    let deduplication_id = options
        .deduplication
        .as_ref()
        .map(|deduplication| deduplication.id.clone());
    let deduplication_expires_at = options
        .deduplication
        .as_ref()
        .and_then(|deduplication| deduplication.ttl)
        .map(|ttl| add_duration(now, ttl));
    let available_at = options.delay.map_or(now, |delay| add_duration(now, delay));
    let rows = execute_query(
        transaction,
        sql_query::<()>(
            "INSERT INTO boot_queue_jobs (queue_name, job_id, job_name, payload_json, \
             options_json, state, priority, lifo, available_at_nanos, attempts_made, \
             stalled_count, deduplication_id, deduplication_expires_at_nanos, \
             created_at_nanos, updated_at_nanos) VALUES (",
        )
        .bind(queue_name)
        .append(", ")
        .bind(job_id)
        .append(", ")
        .bind(name)
        .append(", ")
        .bind(payload_json)
        .append(", ")
        .bind(options_json)
        .append(", 'pending', ")
        .bind(i64::from(options.priority))
        .append(", ")
        .bind(options.lifo)
        .append(", ")
        .bind(available_at)
        .append(", 0, 0, ")
        .bind(deduplication_id)
        .append(", ")
        .bind(deduplication_expires_at)
        .append(", ")
        .bind(now)
        .append(", ")
        .bind(now)
        .append(") ON CONFLICT (queue_name, job_id) DO NOTHING"),
    )
    .await?;
    Ok(rows == 1)
}

pub(super) async fn ensure_idempotent_job(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    name: &str,
    payload_json: &str,
    options_json: &str,
) -> Result<()> {
    let existing = fetch_optional_query(
        transaction,
        sql_query::<(String, String, String)>(
            "SELECT job_name, payload_json, options_json FROM boot_queue_jobs WHERE queue_name = ",
        )
        .bind(queue_name)
        .append(" AND job_id = ")
        .bind(job_id),
    )
    .await?;
    if existing
        .as_ref()
        .is_some_and(|(stored_name, stored_payload, stored_options)| {
            stored_name == name && stored_payload == payload_json && stored_options == options_json
        })
    {
        return Ok(());
    }
    Err(BootError::Conflict(format!(
        "PostgreSQL queue job id {job_id} is already used by different work"
    )))
}

fn validate_job(name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(BootError::BadRequest(
            "PostgreSQL queue job name cannot be empty".to_string(),
        ));
    }
    Ok(name)
}

fn validate_options(options: &QueueJobOptions) -> Result<()> {
    if options
        .job_id
        .as_ref()
        .is_some_and(|id| id.trim().is_empty())
    {
        return Err(BootError::BadRequest(
            "PostgreSQL queue job id cannot be empty".to_string(),
        ));
    }
    if options.timeout.is_some_and(|timeout| timeout.is_zero()) {
        return Err(BootError::BadRequest(
            "PostgreSQL queue timeout must be greater than zero".to_string(),
        ));
    }
    if !options.retry_policy.multiplier.is_finite() || options.retry_policy.multiplier < 0.0 {
        return Err(BootError::BadRequest(
            "PostgreSQL queue retry multiplier must be finite and non-negative".to_string(),
        ));
    }
    if let Some(deduplication) = options.deduplication.as_ref() {
        if deduplication.id.trim().is_empty() {
            return Err(BootError::BadRequest(
                "PostgreSQL queue deduplication id cannot be empty".to_string(),
            ));
        }
        if deduplication.ttl.is_some_and(|ttl| ttl.is_zero()) {
            return Err(BootError::BadRequest(
                "PostgreSQL queue deduplication TTL must be greater than zero".to_string(),
            ));
        }
    }
    if let Some(retention) = options.completion_retention.as_ref() {
        validate_retention("completion retention", retention)?;
    }
    if let Some(retention) = options.failure_retention.as_ref() {
        validate_retention("failure retention", retention)?;
    }
    if options.repeat.is_some()
        || options.ignore_dependency_on_failure
        || options.remove_dependency_on_failure
        || options.continue_parent_on_failure
        || options.fail_parent_on_failure
    {
        return Err(BootError::NotImplemented(
            "PostgreSQL Boot queues do not support Lane flow or repeat options".to_string(),
        ));
    }
    Ok(())
}

fn validate_retention(label: &str, retention: &QueueJobRetention) -> Result<()> {
    if retention.age.is_none() && retention.count.is_none() {
        return Err(BootError::BadRequest(format!(
            "PostgreSQL queue {label} must specify an age or count"
        )));
    }
    if retention.age.is_some_and(|age| age.is_zero()) {
        return Err(BootError::BadRequest(format!(
            "PostgreSQL queue {label} age must be greater than zero"
        )));
    }
    if retention.limit == Some(0) {
        return Err(BootError::BadRequest(format!(
            "PostgreSQL queue {label} limit must be greater than zero"
        )));
    }
    Ok(())
}

fn claimed_job_from_row(row: ClaimedJobRow) -> Result<ClaimedJob> {
    Ok(ClaimedJob {
        id: row.id,
        name: row.name,
        payload: serde_json::from_str(&row.payload_json).map_err(stored_json_error)?,
        options: serde_json::from_str(&row.options_json).map_err(stored_json_error)?,
        lock_token: row.lock_token,
    })
}

fn job_info_from_row(row: JobInfoRow) -> Result<QueueJobInfo> {
    let state = match row.state.as_str() {
        "pending" => QueueJobState::Pending,
        "active" => QueueJobState::Active,
        "completed" => QueueJobState::Completed,
        "failed" => QueueJobState::Failed,
        other => {
            return Err(BootError::Internal(format!(
                "stored PostgreSQL queue state is invalid: {other}"
            )))
        }
    };
    Ok(QueueJobInfo {
        id: row.id,
        name: row.name,
        state,
        data: serde_json::from_str(&row.payload_json).map_err(stored_json_error)?,
    })
}

pub(super) fn require_fenced_row(rows: u64, job_id: &str, action: &str) -> Result<()> {
    if rows == 1 {
        return Ok(());
    }
    Err(lease_conflict(job_id, action))
}

fn lease_conflict(job_id: &str, action: &str) -> BootError {
    BootError::Conflict(format!(
        "PostgreSQL queue job {job_id} lost its lease before {action}"
    ))
}

fn now_nanos() -> i64 {
    Utc::now().timestamp_nanos_opt().unwrap_or(i64::MAX)
}

pub(super) fn add_duration(value: i64, duration: Duration) -> i64 {
    let nanos = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
    value.saturating_add(nanos)
}

pub(super) fn subtract_duration(value: i64, duration: Duration) -> i64 {
    let nanos = i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX);
    value.saturating_sub(nanos)
}

fn json_error(error: serde_json::Error) -> BootError {
    BootError::BadRequest(format!("could not encode PostgreSQL queue job: {error}"))
}

pub(super) fn stored_json_error(error: serde_json::Error) -> BootError {
    BootError::Internal(format!("stored PostgreSQL queue job is invalid: {error}"))
}

fn map_transaction<T>(
    result: std::result::Result<T, PostgresTransactionError<BootError>>,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(PostgresTransactionError::Operation(error)) => Err(error),
        Err(error) => Err(BootError::Internal(format!(
            "PostgreSQL queue transaction failed: {error}"
        ))),
    }
}

pub(super) async fn execute_query<E>(executor: &E, query: SqlQuery<()>) -> Result<u64>
where
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query.compile(&PostgresDialect).map_err(query_error)?;
    Ok(executor
        .execute(&query)
        .await
        .map_err(database_error)?
        .rows_affected)
}

async fn fetch_all_query<T, E>(executor: &E, query: SqlQuery<T>) -> Result<Vec<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let query = query.compile(&PostgresDialect).map_err(query_error)?;
    executor
        .fetch_all(&query)
        .await
        .map_err(database_error)?
        .rows
        .iter()
        .map(T::from_row)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(decode_error)
}

pub(super) async fn fetch_optional_query<T, E>(
    executor: &E,
    query: SqlQuery<T>,
) -> Result<Option<T>>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    let mut rows = fetch_all_query(executor, query).await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(rows.pop()),
        actual => Err(BootError::Internal(format!(
            "PostgreSQL queue returned {actual} rows where at most one was expected"
        ))),
    }
}

pub(super) async fn fetch_one_query<T, E>(executor: &E, query: SqlQuery<T>) -> Result<T>
where
    T: FromRow + Send,
    E: Executor<Row = PostgresRow, Error = PostgresError>,
{
    fetch_optional_query(executor, query)
        .await?
        .ok_or_else(|| BootError::Internal("PostgreSQL queue returned no row".to_string()))
}

pub(super) fn decode<T: FromValue>(
    row: &impl Row,
    index: usize,
) -> std::result::Result<T, DecodeError> {
    let value = row
        .value(index)
        .ok_or(DecodeError::MissingColumn { index })?;
    T::from_value(value, index)
}

fn query_error(error: a3s_orm::Error) -> BootError {
    BootError::Internal(format!("PostgreSQL queue query build failed: {error}"))
}

fn database_error(error: PostgresError) -> BootError {
    BootError::Internal(format!("PostgreSQL queue storage failed: {error}"))
}

fn decode_error(error: DecodeError) -> BootError {
    BootError::Internal(format!("PostgreSQL queue row decoding failed: {error}"))
}
