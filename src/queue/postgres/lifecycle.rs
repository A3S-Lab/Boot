use a3s_orm::{sql_query, DecodeError, FromRow, PostgresTransaction, Row};

use crate::Result;

use super::super::QueueJobOptions;
use super::store::{
    decode, ensure_idempotent_job, execute_query, insert_job, require_fenced_row,
    stored_json_error, subtract_duration,
};

pub(super) struct ActiveJobRow {
    pub options_json: String,
    pub attempts_made: u32,
    pub successor_job_id: Option<String>,
    pub successor_job_name: Option<String>,
    pub successor_payload_json: Option<String>,
    pub successor_options_json: Option<String>,
}

impl FromRow for ActiveJobRow {
    fn from_row(row: &impl Row) -> std::result::Result<Self, DecodeError> {
        Ok(Self {
            options_json: decode(row, 0)?,
            attempts_made: decode(row, 1)?,
            successor_job_id: decode(row, 2)?,
            successor_job_name: decode(row, 3)?,
            successor_payload_json: decode(row, 4)?,
            successor_options_json: decode(row, 5)?,
        })
    }
}

pub(super) struct ExpiredJobRow {
    pub id: String,
    pub lock_token: String,
    pub options_json: String,
    pub stalled_count: u32,
    pub successor_job_id: Option<String>,
    pub successor_job_name: Option<String>,
    pub successor_payload_json: Option<String>,
    pub successor_options_json: Option<String>,
}

impl FromRow for ExpiredJobRow {
    fn from_row(row: &impl Row) -> std::result::Result<Self, DecodeError> {
        Ok(Self {
            id: decode(row, 0)?,
            lock_token: decode(row, 1)?,
            options_json: decode(row, 2)?,
            stalled_count: decode(row, 3)?,
            successor_job_id: decode(row, 4)?,
            successor_job_name: decode(row, 5)?,
            successor_payload_json: decode(row, 6)?,
            successor_options_json: decode(row, 7)?,
        })
    }
}

#[derive(Clone)]
pub(super) struct Successor {
    id: String,
    name: String,
    payload_json: String,
    options_json: String,
}

pub(super) fn successor_from_active(row: &ActiveJobRow) -> Option<Successor> {
    successor(
        row.successor_job_id.clone(),
        row.successor_job_name.clone(),
        row.successor_payload_json.clone(),
        row.successor_options_json.clone(),
    )
}

pub(super) fn successor_from_expired(row: &ExpiredJobRow) -> Option<Successor> {
    successor(
        row.successor_job_id.clone(),
        row.successor_job_name.clone(),
        row.successor_payload_json.clone(),
        row.successor_options_json.clone(),
    )
}

fn successor(
    id: Option<String>,
    name: Option<String>,
    payload_json: Option<String>,
    options_json: Option<String>,
) -> Option<Successor> {
    match (id, name, payload_json, options_json) {
        (Some(id), Some(name), Some(payload_json), Some(options_json)) => Some(Successor {
            id,
            name,
            payload_json,
            options_json,
        }),
        _ => None,
    }
}

pub(super) async fn terminal_completion(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    lock_token: &str,
    options: &QueueJobOptions,
    successor: Option<Successor>,
    now: i64,
) -> Result<()> {
    let remove = options.remove_on_complete
        || options
            .completion_retention
            .as_ref()
            .is_some_and(|retention| retention.count == Some(0));
    set_terminal(
        transaction,
        queue_name,
        job_id,
        lock_token,
        "completed",
        None,
        remove,
        successor,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn terminal_failure(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    lock_token: &str,
    options: &QueueJobOptions,
    message: &str,
    successor: Option<Successor>,
    now: i64,
) -> Result<()> {
    let remove = options.remove_on_fail
        || options
            .failure_retention
            .as_ref()
            .is_some_and(|retention| retention.count == Some(0));
    set_terminal(
        transaction,
        queue_name,
        job_id,
        lock_token,
        "failed",
        Some(message),
        remove,
        successor,
        now,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn set_terminal(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    lock_token: &str,
    state: &str,
    failure: Option<&str>,
    remove: bool,
    successor: Option<Successor>,
    now: i64,
) -> Result<()> {
    let rows = if remove {
        execute_query(
            transaction,
            sql_query::<()>("DELETE FROM boot_queue_jobs WHERE queue_name = ")
                .bind(queue_name)
                .append(" AND job_id = ")
                .bind(job_id)
                .append(" AND state = 'active' AND lock_token = ")
                .bind(lock_token),
        )
        .await?
    } else {
        execute_query(
            transaction,
            sql_query::<()>("UPDATE boot_queue_jobs SET state = ")
                .bind(state)
                .append(
                    ", worker_id = NULL, lock_token = NULL, lease_expires_at_nanos = NULL, \
                     failed_reason = ",
                )
                .bind(failure)
                .append(
                    ", deduplication_id = NULL, deduplication_expires_at_nanos = NULL, \
                     successor_job_id = NULL, successor_job_name = NULL, \
                     successor_payload_json = NULL, successor_options_json = NULL, \
                     finished_at_nanos = ",
                )
                .bind(now)
                .append(", updated_at_nanos = ")
                .bind(now)
                .append(" WHERE queue_name = ")
                .bind(queue_name)
                .append(" AND job_id = ")
                .bind(job_id)
                .append(" AND state = 'active' AND lock_token = ")
                .bind(lock_token),
        )
        .await?
    };
    require_fenced_row(rows, job_id, state)?;
    if let Some(successor) = successor {
        materialize_successor(transaction, queue_name, successor, now).await?;
    }
    Ok(())
}

async fn materialize_successor(
    transaction: &PostgresTransaction,
    queue_name: &str,
    successor: Successor,
    now: i64,
) -> Result<()> {
    let options: QueueJobOptions =
        serde_json::from_str(&successor.options_json).map_err(stored_json_error)?;
    let inserted = insert_job(
        transaction,
        queue_name,
        &successor.id,
        &successor.name,
        &successor.payload_json,
        &successor.options_json,
        &options,
        now,
    )
    .await?;
    if !inserted {
        ensure_idempotent_job(
            transaction,
            queue_name,
            &successor.id,
            &successor.name,
            &successor.payload_json,
            &successor.options_json,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn apply_retention(
    transaction: &PostgresTransaction,
    queue_name: &str,
    state: &str,
    options: &QueueJobOptions,
    now: i64,
) -> Result<()> {
    let retention = match state {
        "completed" if !options.remove_on_complete => options.completion_retention.as_ref(),
        "failed" if !options.remove_on_fail => options.failure_retention.as_ref(),
        _ => None,
    };
    let Some(retention) = retention else {
        return Ok(());
    };
    if let Some(age) = retention.age {
        let limit = i64::try_from(retention.limit.unwrap_or(usize::MAX)).unwrap_or(i64::MAX);
        execute_query(
            transaction,
            sql_query::<()>(
                "DELETE FROM boot_queue_jobs WHERE (queue_name, job_id) IN (SELECT queue_name, \
                 job_id FROM boot_queue_jobs WHERE queue_name = ",
            )
            .bind(queue_name)
            .append(" AND state = ")
            .bind(state)
            .append(" AND finished_at_nanos < ")
            .bind(subtract_duration(now, age))
            .append(" ORDER BY finished_at_nanos ASC, job_id ASC LIMIT ")
            .bind(limit)
            .append(")"),
        )
        .await?;
    }
    if let Some(count) = retention.count {
        let offset = i64::try_from(count).unwrap_or(i64::MAX);
        let limit = i64::try_from(retention.limit.unwrap_or(usize::MAX)).unwrap_or(i64::MAX);
        execute_query(
            transaction,
            sql_query::<()>(
                "DELETE FROM boot_queue_jobs WHERE (queue_name, job_id) IN (SELECT queue_name, \
                 job_id FROM boot_queue_jobs WHERE queue_name = ",
            )
            .bind(queue_name)
            .append(" AND state = ")
            .bind(state)
            .append(" ORDER BY finished_at_nanos DESC, job_id DESC OFFSET ")
            .bind(offset)
            .append(" LIMIT ")
            .bind(limit)
            .append(")"),
        )
        .await?;
    }
    Ok(())
}
