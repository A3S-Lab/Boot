use crate::{BootError, BoxFuture, Module, ProviderDefinition, ProviderToken, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, RwLock};

/// One database statement plus serialized positional parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseStatement {
    sql: String,
    params: Vec<Value>,
}

impl DatabaseStatement {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            params: Vec::new(),
        }
    }

    pub fn with_param<T>(self, value: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!("failed to serialize database parameter: {error}"))
        })?;
        Ok(self.with_param_value(value))
    }

    pub fn with_param_value(mut self, value: Value) -> Self {
        self.params.push(value);
        self
    }

    pub fn with_params<I>(mut self, params: I) -> Self
    where
        I: IntoIterator<Item = Value>,
    {
        self.params.extend(params);
        self
    }

    pub fn sql(&self) -> &str {
        &self.sql
    }

    pub fn params(&self) -> &[Value] {
        &self.params
    }
}

/// One adapter-neutral database row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DatabaseRow {
    values: BTreeMap<String, Value>,
}

impl DatabaseRow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with<T>(self, key: impl Into<String>, value: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize database row value `{key}`: {error}"
            ))
        })?;
        Ok(self.with_value(key, value))
    }

    pub fn with_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    pub fn value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    pub fn get<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.values.get(key) else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                BootError::Internal(format!("invalid database row value `{key}`: {error}"))
            })
    }

    pub fn values(&self) -> &BTreeMap<String, Value> {
        &self.values
    }
}

/// Result for a write statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseResult {
    rows_affected: u64,
    last_insert_id: Option<String>,
}

impl DatabaseResult {
    pub fn new(rows_affected: u64) -> Self {
        Self {
            rows_affected,
            last_insert_id: None,
        }
    }

    pub fn with_last_insert_id(mut self, id: impl Into<String>) -> Self {
        self.last_insert_id = Some(id.into());
        self
    }

    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }

    pub fn last_insert_id(&self) -> Option<&str> {
        self.last_insert_id.as_deref()
    }
}

/// Backend used by [`Database`] to execute statements.
pub trait DatabaseBackend: Send + Sync + 'static {
    fn execute(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<DatabaseResult>>;

    fn query(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<Vec<DatabaseRow>>>;

    fn begin(&self) -> BoxFuture<'static, Result<Arc<dyn DatabaseTransactionBackend>>>;
}

/// Backend handle for one transaction.
pub trait DatabaseTransactionBackend: Send + Sync + 'static {
    fn execute(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<DatabaseResult>>;

    fn query(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<Vec<DatabaseRow>>>;

    fn commit(&self) -> BoxFuture<'static, Result<()>>;

    fn rollback(&self) -> BoxFuture<'static, Result<()>>;
}

/// Injectable database facade, comparable to a Nest database provider.
#[derive(Clone)]
pub struct Database {
    backend: Arc<dyn DatabaseBackend>,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub fn new<B>(backend: B) -> Self
    where
        B: DatabaseBackend,
    {
        Self::from_backend_arc(Arc::new(backend))
    }

    pub fn from_backend_arc(backend: Arc<dyn DatabaseBackend>) -> Self {
        Self { backend }
    }

    pub fn in_memory() -> Self {
        Self::new(InMemoryDatabaseBackend::new())
    }

    pub async fn execute<I>(&self, sql: impl Into<String>, params: I) -> Result<DatabaseResult>
    where
        I: IntoIterator<Item = Value>,
    {
        self.execute_statement(DatabaseStatement::new(sql).with_params(params))
            .await
    }

    pub async fn execute_statement(&self, statement: DatabaseStatement) -> Result<DatabaseResult> {
        self.backend.execute(statement).await
    }

    pub async fn query<I>(&self, sql: impl Into<String>, params: I) -> Result<Vec<DatabaseRow>>
    where
        I: IntoIterator<Item = Value>,
    {
        self.query_statement(DatabaseStatement::new(sql).with_params(params))
            .await
    }

    pub async fn query_statement(&self, statement: DatabaseStatement) -> Result<Vec<DatabaseRow>> {
        self.backend.query(statement).await
    }

    pub async fn transaction<F, Fut, T>(&self, callback: F) -> Result<T>
    where
        F: FnOnce(DatabaseTransaction) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let transaction = DatabaseTransaction::from_backend_arc(self.backend.begin().await?);
        match callback(transaction.clone()).await {
            Ok(value) => {
                transaction.commit().await?;
                Ok(value)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

/// Facade passed into transaction callbacks.
#[derive(Clone)]
pub struct DatabaseTransaction {
    backend: Arc<dyn DatabaseTransactionBackend>,
}

impl fmt::Debug for DatabaseTransaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseTransaction")
            .finish_non_exhaustive()
    }
}

impl DatabaseTransaction {
    pub fn from_backend_arc(backend: Arc<dyn DatabaseTransactionBackend>) -> Self {
        Self { backend }
    }

    pub async fn execute<I>(&self, sql: impl Into<String>, params: I) -> Result<DatabaseResult>
    where
        I: IntoIterator<Item = Value>,
    {
        self.execute_statement(DatabaseStatement::new(sql).with_params(params))
            .await
    }

    pub async fn execute_statement(&self, statement: DatabaseStatement) -> Result<DatabaseResult> {
        self.backend.execute(statement).await
    }

    pub async fn query<I>(&self, sql: impl Into<String>, params: I) -> Result<Vec<DatabaseRow>>
    where
        I: IntoIterator<Item = Value>,
    {
        self.query_statement(DatabaseStatement::new(sql).with_params(params))
            .await
    }

    pub async fn query_statement(&self, statement: DatabaseStatement) -> Result<Vec<DatabaseRow>> {
        self.backend.query(statement).await
    }

    pub async fn commit(&self) -> Result<()> {
        self.backend.commit().await
    }

    pub async fn rollback(&self) -> Result<()> {
        self.backend.rollback().await
    }
}

/// In-memory database backend suitable for tests and local adapter development.
#[derive(Debug, Clone, Default)]
pub struct InMemoryDatabaseBackend {
    state: Arc<RwLock<InMemoryDatabaseState>>,
}

#[derive(Debug, Clone, Default)]
struct InMemoryDatabaseState {
    executed: Vec<DatabaseStatement>,
    queried: Vec<DatabaseStatement>,
    query_results: BTreeMap<String, Vec<DatabaseRow>>,
    transactions: Vec<InMemoryDatabaseTransactionLog>,
    next_transaction_id: u64,
}

impl InMemoryDatabaseBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_query_result<I>(self, sql: impl Into<String>, rows: I) -> Result<Self>
    where
        I: IntoIterator<Item = DatabaseRow>,
    {
        self.set_query_result(sql, rows)?;
        Ok(self)
    }

    pub fn set_query_result<I>(&self, sql: impl Into<String>, rows: I) -> Result<()>
    where
        I: IntoIterator<Item = DatabaseRow>,
    {
        self.write_state()?
            .query_results
            .insert(sql.into(), rows.into_iter().collect());
        Ok(())
    }

    pub fn executed(&self) -> Result<Vec<DatabaseStatement>> {
        Ok(self.read_state()?.executed.clone())
    }

    pub fn queried(&self) -> Result<Vec<DatabaseStatement>> {
        Ok(self.read_state()?.queried.clone())
    }

    pub fn transactions(&self) -> Result<Vec<InMemoryDatabaseTransactionLog>> {
        Ok(self.read_state()?.transactions.clone())
    }

    fn begin_transaction(&self) -> Result<InMemoryDatabaseTransactionBackend> {
        let mut state = self.write_state()?;
        state.next_transaction_id += 1;
        Ok(InMemoryDatabaseTransactionBackend {
            id: state.next_transaction_id,
            state: Arc::clone(&self.state),
            pending: Arc::new(RwLock::new(Vec::new())),
            finished: Arc::new(RwLock::new(false)),
        })
    }

    fn read_state(&self) -> Result<std::sync::RwLockReadGuard<'_, InMemoryDatabaseState>> {
        self.state
            .read()
            .map_err(|_| BootError::Internal("database state lock is poisoned".to_string()))
    }

    fn write_state(&self) -> Result<std::sync::RwLockWriteGuard<'_, InMemoryDatabaseState>> {
        self.state
            .write()
            .map_err(|_| BootError::Internal("database state lock is poisoned".to_string()))
    }
}

impl DatabaseBackend for InMemoryDatabaseBackend {
    fn execute(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<DatabaseResult>> {
        let backend = self.clone();
        Box::pin(async move {
            backend.write_state()?.executed.push(statement);
            Ok(DatabaseResult::new(1))
        })
    }

    fn query(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<Vec<DatabaseRow>>> {
        let backend = self.clone();
        Box::pin(async move {
            let mut state = backend.write_state()?;
            state.queried.push(statement.clone());
            Ok(state
                .query_results
                .get(statement.sql())
                .cloned()
                .unwrap_or_default())
        })
    }

    fn begin(&self) -> BoxFuture<'static, Result<Arc<dyn DatabaseTransactionBackend>>> {
        let backend = self.clone();
        Box::pin(async move {
            Ok(Arc::new(backend.begin_transaction()?) as Arc<dyn DatabaseTransactionBackend>)
        })
    }
}

/// Completed in-memory transaction record.
#[derive(Debug, Clone, PartialEq)]
pub struct InMemoryDatabaseTransactionLog {
    id: u64,
    committed: bool,
    rolled_back: bool,
    statements: Vec<DatabaseStatement>,
}

impl InMemoryDatabaseTransactionLog {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn committed(&self) -> bool {
        self.committed
    }

    pub fn rolled_back(&self) -> bool {
        self.rolled_back
    }

    pub fn statements(&self) -> &[DatabaseStatement] {
        &self.statements
    }
}

#[derive(Debug, Clone)]
struct InMemoryDatabaseTransactionBackend {
    id: u64,
    state: Arc<RwLock<InMemoryDatabaseState>>,
    pending: Arc<RwLock<Vec<DatabaseStatement>>>,
    finished: Arc<RwLock<bool>>,
}

impl InMemoryDatabaseTransactionBackend {
    fn check_open(&self) -> Result<()> {
        if *self
            .finished
            .read()
            .map_err(|_| BootError::Internal("database transaction lock is poisoned".to_string()))?
        {
            return Err(BootError::Internal(
                "database transaction is already finished".to_string(),
            ));
        }
        Ok(())
    }

    fn query_result(&self, sql: &str) -> Result<Vec<DatabaseRow>> {
        Ok(self
            .state
            .read()
            .map_err(|_| BootError::Internal("database state lock is poisoned".to_string()))?
            .query_results
            .get(sql)
            .cloned()
            .unwrap_or_default())
    }

    fn push_pending(&self, statement: DatabaseStatement) -> Result<()> {
        self.check_open()?;
        self.pending
            .write()
            .map_err(|_| BootError::Internal("database transaction lock is poisoned".to_string()))?
            .push(statement);
        Ok(())
    }

    fn finish(&self, committed: bool) -> Result<()> {
        let mut finished = self.finished.write().map_err(|_| {
            BootError::Internal("database transaction lock is poisoned".to_string())
        })?;
        if *finished {
            return Err(BootError::Internal(
                "database transaction is already finished".to_string(),
            ));
        }
        *finished = true;

        let statements = self
            .pending
            .read()
            .map_err(|_| BootError::Internal("database transaction lock is poisoned".to_string()))?
            .clone();
        self.state
            .write()
            .map_err(|_| BootError::Internal("database state lock is poisoned".to_string()))?
            .transactions
            .push(InMemoryDatabaseTransactionLog {
                id: self.id,
                committed,
                rolled_back: !committed,
                statements,
            });
        Ok(())
    }
}

impl DatabaseTransactionBackend for InMemoryDatabaseTransactionBackend {
    fn execute(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<DatabaseResult>> {
        let transaction = self.clone();
        Box::pin(async move {
            transaction.push_pending(statement)?;
            Ok(DatabaseResult::new(1))
        })
    }

    fn query(&self, statement: DatabaseStatement) -> BoxFuture<'static, Result<Vec<DatabaseRow>>> {
        let transaction = self.clone();
        Box::pin(async move {
            transaction.push_pending(statement.clone())?;
            transaction.query_result(statement.sql())
        })
    }

    fn commit(&self) -> BoxFuture<'static, Result<()>> {
        let transaction = self.clone();
        Box::pin(async move { transaction.finish(true) })
    }

    fn rollback(&self) -> BoxFuture<'static, Result<()>> {
        let transaction = self.clone();
        Box::pin(async move { transaction.finish(false) })
    }
}

/// Module that registers and exports a [`Database`] provider.
#[derive(Clone)]
pub struct DatabaseModule {
    name: &'static str,
    token: ProviderToken,
    database: Arc<Database>,
    global: bool,
}

impl fmt::Debug for DatabaseModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DatabaseModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl DatabaseModule {
    pub fn in_memory(name: &'static str) -> Self {
        Self::from_database(name, Database::in_memory())
    }

    pub fn from_backend<B>(name: &'static str, backend: B) -> Self
    where
        B: DatabaseBackend,
    {
        Self::from_database(name, Database::new(backend))
    }

    pub fn from_backend_arc(name: &'static str, backend: Arc<dyn DatabaseBackend>) -> Self {
        Self::from_database(name, Database::from_backend_arc(backend))
    }

    pub fn from_database(name: &'static str, database: Database) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Database>(),
            database: Arc::new(database),
            global: false,
        }
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for DatabaseModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.database),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}
