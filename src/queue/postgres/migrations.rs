use a3s_orm::Migration;

const POSTGRES_QUEUE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS boot_queue_jobs (
    queue_name TEXT NOT NULL,
    job_id TEXT NOT NULL,
    job_name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    options_json TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'active', 'completed', 'failed')),
    priority BIGINT NOT NULL CHECK (priority BETWEEN 0 AND 4294967295),
    lifo BOOLEAN NOT NULL,
    available_at_nanos BIGINT NOT NULL,
    attempts_made BIGINT NOT NULL DEFAULT 0 CHECK (attempts_made >= 0),
    stalled_count BIGINT NOT NULL DEFAULT 0 CHECK (stalled_count >= 0),
    worker_id TEXT,
    lock_token TEXT,
    lease_expires_at_nanos BIGINT,
    failed_reason TEXT,
    deduplication_id TEXT,
    deduplication_expires_at_nanos BIGINT,
    successor_job_id TEXT,
    successor_job_name TEXT,
    successor_payload_json TEXT,
    successor_options_json TEXT,
    created_at_nanos BIGINT NOT NULL,
    updated_at_nanos BIGINT NOT NULL,
    finished_at_nanos BIGINT,
    PRIMARY KEY (queue_name, job_id),
    CHECK (
        (state = 'active' AND worker_id IS NOT NULL AND lock_token IS NOT NULL
            AND lease_expires_at_nanos IS NOT NULL)
        OR
        (state <> 'active' AND worker_id IS NULL AND lock_token IS NULL
            AND lease_expires_at_nanos IS NULL)
    ),
    CHECK (
        (successor_job_id IS NULL AND successor_job_name IS NULL
            AND successor_payload_json IS NULL AND successor_options_json IS NULL)
        OR
        (successor_job_id IS NOT NULL AND successor_job_name IS NOT NULL
            AND successor_payload_json IS NOT NULL AND successor_options_json IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS boot_queue_jobs_claim_idx
    ON boot_queue_jobs (
        queue_name,
        state,
        available_at_nanos,
        priority,
        created_at_nanos,
        job_id
    );

CREATE INDEX IF NOT EXISTS boot_queue_jobs_lease_idx
    ON boot_queue_jobs (queue_name, state, lease_expires_at_nanos, job_id);

CREATE INDEX IF NOT EXISTS boot_queue_jobs_terminal_idx
    ON boot_queue_jobs (queue_name, state, finished_at_nanos DESC, job_id DESC);

CREATE UNIQUE INDEX IF NOT EXISTS boot_queue_jobs_active_dedup_idx
    ON boot_queue_jobs (queue_name, deduplication_id)
    WHERE deduplication_id IS NOT NULL AND state IN ('pending', 'active');
"#;

pub(super) fn postgres_queue_migrations() -> Vec<Migration> {
    vec![Migration::new(
        "a3s-boot-0001-queue",
        "create durable Boot queue jobs",
        POSTGRES_QUEUE_SQL,
    )]
}
