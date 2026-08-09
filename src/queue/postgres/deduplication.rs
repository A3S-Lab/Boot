use a3s_orm::{sql_query, PostgresTransaction};

use crate::Result;

use super::super::QueueJobOptions;
use super::store::{add_duration, execute_query, fetch_one_query, fetch_optional_query};

pub(super) async fn lock_deduplication(
    transaction: &PostgresTransaction,
    queue_name: &str,
    deduplication_id: &str,
) -> Result<()> {
    let key = format!("a3s-boot:{queue_name}:{deduplication_id}");
    fetch_one_query(
        transaction,
        sql_query::<i32>("SELECT 1 FROM pg_advisory_xact_lock(hashtextextended(")
            .bind(key)
            .append(", 0))"),
    )
    .await?;
    Ok(())
}

pub(super) async fn release_expired_deduplication(
    transaction: &PostgresTransaction,
    queue_name: &str,
    deduplication_id: &str,
    now: i64,
) -> Result<()> {
    execute_query(
        transaction,
        sql_query::<()>(
            "UPDATE boot_queue_jobs SET deduplication_id = NULL, \
             deduplication_expires_at_nanos = NULL, updated_at_nanos = ",
        )
        .bind(now)
        .append(" WHERE queue_name = ")
        .bind(queue_name)
        .append(" AND deduplication_id = ")
        .bind(deduplication_id)
        .append(" AND state IN ('pending', 'active') AND deduplication_expires_at_nanos <= ")
        .bind(now),
    )
    .await?;
    Ok(())
}

pub(super) async fn find_deduplication_owner(
    transaction: &PostgresTransaction,
    queue_name: &str,
    deduplication_id: &str,
) -> Result<Option<(String, String, String, i64)>> {
    fetch_optional_query(
        transaction,
        sql_query::<(String, String, String, i64)>(
            "SELECT job_id, job_name, state, available_at_nanos FROM boot_queue_jobs \
             WHERE queue_name = ",
        )
        .bind(queue_name)
        .append(" AND deduplication_id = ")
        .bind(deduplication_id)
        .append(" AND state IN ('pending', 'active') FOR UPDATE"),
    )
    .await
}

pub(super) async fn update_deduplication_expiry(
    transaction: &PostgresTransaction,
    queue_name: &str,
    job_id: &str,
    expires_at: Option<i64>,
    now: i64,
) -> Result<()> {
    execute_query(
        transaction,
        sql_query::<()>("UPDATE boot_queue_jobs SET deduplication_expires_at_nanos = ")
            .bind(expires_at)
            .append(", updated_at_nanos = ")
            .bind(now)
            .append(" WHERE queue_name = ")
            .bind(queue_name)
            .append(" AND job_id = ")
            .bind(job_id),
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn store_successor(
    transaction: &PostgresTransaction,
    queue_name: &str,
    owner_id: &str,
    job_id: &str,
    name: &str,
    payload_json: &str,
    options_json: &str,
    now: i64,
) -> Result<()> {
    execute_query(
        transaction,
        sql_query::<()>("UPDATE boot_queue_jobs SET successor_job_id = ")
            .bind(job_id)
            .append(", successor_job_name = ")
            .bind(name)
            .append(", successor_payload_json = ")
            .bind(payload_json)
            .append(", successor_options_json = ")
            .bind(options_json)
            .append(", updated_at_nanos = ")
            .bind(now)
            .append(" WHERE queue_name = ")
            .bind(queue_name)
            .append(" AND job_id = ")
            .bind(owner_id)
            .append(" AND state = 'active'"),
    )
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn replace_delayed_owner(
    transaction: &PostgresTransaction,
    queue_name: &str,
    owner_id: &str,
    name: &str,
    payload_json: &str,
    options_json: &str,
    options: &QueueJobOptions,
    now: i64,
) -> Result<()> {
    execute_query(
        transaction,
        sql_query::<()>("UPDATE boot_queue_jobs SET job_name = ")
            .bind(name)
            .append(", payload_json = ")
            .bind(payload_json)
            .append(", options_json = ")
            .bind(options_json)
            .append(", priority = ")
            .bind(i64::from(options.priority))
            .append(", lifo = ")
            .bind(options.lifo)
            .append(", available_at_nanos = ")
            .bind(options.delay.map_or(now, |delay| add_duration(now, delay)))
            .append(", updated_at_nanos = ")
            .bind(now)
            .append(" WHERE queue_name = ")
            .bind(queue_name)
            .append(" AND job_id = ")
            .bind(owner_id)
            .append(" AND state = 'pending'"),
    )
    .await?;
    Ok(())
}
