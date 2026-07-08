use crate::{
    BootError, BoxFuture, ExecutionContext, Guard, Module, ModuleRef, ProviderDefinition,
    ProviderToken, Result,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, RwLock};

pub const AUTH_PUBLIC_METADATA: &str = "auth.public";
pub const AUTH_ROLES_METADATA: &str = "auth.roles";
pub const AUTH_SCOPES_METADATA: &str = "auth.scopes";
pub const AUTH_STRATEGY_METADATA: &str = "auth.strategy";

/// Authenticated request identity attached by [`AuthGuard`].
#[derive(Debug, Clone, PartialEq)]
pub struct AuthPrincipal {
    subject: String,
    strategy: String,
    claims: BTreeMap<String, Value>,
    roles: BTreeSet<String>,
    scopes: BTreeSet<String>,
}

impl AuthPrincipal {
    pub fn new(subject: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            strategy: String::new(),
            claims: BTreeMap::new(),
            roles: BTreeSet::new(),
            scopes: BTreeSet::new(),
        }
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn strategy(&self) -> &str {
        &self.strategy
    }

    pub fn claims(&self) -> &BTreeMap<String, Value> {
        &self.claims
    }

    pub fn claim(&self, key: &str) -> Option<&Value> {
        self.claims.get(key)
    }

    pub fn roles(&self) -> impl Iterator<Item = &str> {
        self.roles.iter().map(String::as_str)
    }

    pub fn scopes(&self) -> impl Iterator<Item = &str> {
        self.scopes.iter().map(String::as_str)
    }

    pub fn with_strategy(mut self, strategy: impl Into<String>) -> Self {
        self.strategy = strategy.into();
        self
    }

    pub fn with_claim<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!("failed to serialize auth claim `{key}`: {error}"))
        })?;
        Ok(self.with_claim_value(key, value))
    }

    pub fn with_claim_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.claims.insert(key.into(), value);
        self
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.insert(role.into());
        self
    }

    pub fn with_roles<I, R>(mut self, roles: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: Into<String>,
    {
        self.roles.extend(roles.into_iter().map(Into::into));
        self
    }

    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }

    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.insert(scope.into());
        self
    }

    pub fn with_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.scopes.extend(scopes.into_iter().map(Into::into));
        self
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.contains(scope)
    }

    fn with_default_strategy(mut self, strategy: &str) -> Self {
        if self.strategy.is_empty() {
            self.strategy = strategy.to_string();
        }
        self
    }
}

/// Parsed authorization credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCredentials {
    scheme: String,
    token: String,
}

impl AuthCredentials {
    pub fn new(scheme: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            scheme: scheme.into(),
            token: token.into(),
        }
    }

    pub fn bearer(token: impl Into<String>) -> Self {
        Self::new("Bearer", token)
    }

    pub fn scheme(&self) -> &str {
        &self.scheme
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

/// Strategy that authenticates one execution context.
pub trait AuthStrategy: Send + Sync + 'static {
    fn authenticate(
        &self,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>>;
}

impl<F, Fut> AuthStrategy for F
where
    F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<AuthPrincipal>>> + Send + 'static,
{
    fn authenticate(
        &self,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>> {
        Box::pin(self(context))
    }
}

/// Verifies a bearer token and returns an authenticated principal.
pub trait BearerTokenVerifier: Send + Sync + 'static {
    fn verify(
        &self,
        token: String,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>>;
}

impl<F, Fut> BearerTokenVerifier for F
where
    F: Fn(String, ExecutionContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<AuthPrincipal>>> + Send + 'static,
{
    fn verify(
        &self,
        token: String,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>> {
        Box::pin(self(token, context))
    }
}

/// Bearer-token auth strategy, useful for JWT or opaque-token verification.
#[derive(Clone)]
pub struct BearerAuthStrategy {
    verifier: Arc<dyn BearerTokenVerifier>,
}

impl fmt::Debug for BearerAuthStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BearerAuthStrategy").finish_non_exhaustive()
    }
}

impl BearerAuthStrategy {
    pub fn new<V>(verifier: V) -> Self
    where
        V: BearerTokenVerifier,
    {
        Self {
            verifier: Arc::new(verifier),
        }
    }

    pub fn from_arc(verifier: Arc<dyn BearerTokenVerifier>) -> Self {
        Self { verifier }
    }
}

impl AuthStrategy for BearerAuthStrategy {
    fn authenticate(
        &self,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<AuthPrincipal>>> {
        let verifier = Arc::clone(&self.verifier);
        Box::pin(async move {
            let Some(token) = context.request.bearer_token().map(ToString::to_string) else {
                return Ok(None);
            };
            verifier.verify(token, context).await
        })
    }
}

/// Named strategy registration consumed by [`AuthModule`].
#[derive(Clone)]
pub struct AuthStrategyDefinition {
    name: String,
    strategy: Arc<dyn AuthStrategy>,
}

impl fmt::Debug for AuthStrategyDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthStrategyDefinition")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl AuthStrategyDefinition {
    pub fn new<S>(name: impl Into<String>, strategy: S) -> Self
    where
        S: AuthStrategy,
    {
        Self::from_arc(name, Arc::new(strategy))
    }

    pub fn from_arc(name: impl Into<String>, strategy: Arc<dyn AuthStrategy>) -> Self {
        Self {
            name: name.into(),
            strategy,
        }
    }

    pub fn bearer<V>(name: impl Into<String>, verifier: V) -> Self
    where
        V: BearerTokenVerifier,
    {
        Self::new(name, BearerAuthStrategy::new(verifier))
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Provider-backed strategy registry used by [`AuthGuard`].
#[derive(Clone)]
pub struct AuthService {
    default_strategy: Arc<RwLock<String>>,
    strategies: Arc<RwLock<BTreeMap<String, Arc<dyn AuthStrategy>>>>,
}

impl fmt::Debug for AuthService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let strategy_count = self.strategies.read().map(|items| items.len()).unwrap_or(0);
        f.debug_struct("AuthService")
            .field(
                "default_strategy",
                &self.default_strategy().unwrap_or_default(),
            )
            .field("strategies", &strategy_count)
            .finish_non_exhaustive()
    }
}

impl Default for AuthService {
    fn default() -> Self {
        Self::new("bearer")
    }
}

impl AuthService {
    pub fn new(default_strategy: impl Into<String>) -> Self {
        Self {
            default_strategy: Arc::new(RwLock::new(default_strategy.into())),
            strategies: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    pub fn default_strategy(&self) -> Result<String> {
        Ok(self.read_default_strategy()?.clone())
    }

    pub fn set_default_strategy(&self, strategy: impl Into<String>) -> Result<()> {
        *self.write_default_strategy()? = strategy.into();
        Ok(())
    }

    pub fn register_strategy<S>(&self, name: impl Into<String>, strategy: S) -> Result<()>
    where
        S: AuthStrategy,
    {
        self.register_strategy_definition(AuthStrategyDefinition::new(name, strategy))
    }

    pub fn register_bearer_strategy<V>(&self, name: impl Into<String>, verifier: V) -> Result<()>
    where
        V: BearerTokenVerifier,
    {
        self.register_strategy_definition(AuthStrategyDefinition::bearer(name, verifier))
    }

    pub fn register_strategy_definition(&self, definition: AuthStrategyDefinition) -> Result<()> {
        if definition.name.trim().is_empty() {
            return Err(BootError::Internal(
                "auth strategy name cannot be empty".to_string(),
            ));
        }
        let mut strategies = self.write_strategies()?;
        if strategies.contains_key(&definition.name) {
            return Err(BootError::Internal(format!(
                "auth strategy is already registered: {}",
                definition.name
            )));
        }
        strategies.insert(definition.name, definition.strategy);
        Ok(())
    }

    pub async fn authenticate(
        &self,
        strategy_name: &str,
        context: ExecutionContext,
    ) -> Result<Option<AuthPrincipal>> {
        let strategy = self
            .read_strategies()?
            .get(strategy_name)
            .cloned()
            .ok_or_else(|| {
                BootError::Internal(format!("auth strategy is not registered: {strategy_name}"))
            })?;
        let principal = strategy.authenticate(context).await?;
        Ok(principal.map(|principal| principal.with_default_strategy(strategy_name)))
    }

    pub fn strategy_count(&self) -> Result<usize> {
        Ok(self.read_strategies()?.len())
    }

    pub fn clear_strategies(&self) -> Result<()> {
        self.write_strategies()?.clear();
        Ok(())
    }

    fn read_default_strategy(&self) -> Result<std::sync::RwLockReadGuard<'_, String>> {
        self.default_strategy
            .read()
            .map_err(|_| BootError::Internal("auth default strategy lock is poisoned".to_string()))
    }

    fn write_default_strategy(&self) -> Result<std::sync::RwLockWriteGuard<'_, String>> {
        self.default_strategy
            .write()
            .map_err(|_| BootError::Internal("auth default strategy lock is poisoned".to_string()))
    }

    fn read_strategies(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, Arc<dyn AuthStrategy>>>> {
        self.strategies
            .read()
            .map_err(|_| BootError::Internal("auth strategy lock is poisoned".to_string()))
    }

    fn write_strategies(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, Arc<dyn AuthStrategy>>>> {
        self.strategies
            .write()
            .map_err(|_| BootError::Internal("auth strategy lock is poisoned".to_string()))
    }
}

/// Nest-style authentication guard backed by [`AuthService`].
#[derive(Debug, Clone, Default)]
pub struct AuthGuard {
    strategy: Option<String>,
    required_roles: BTreeSet<String>,
    required_scopes: BTreeSet<String>,
}

impl AuthGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn strategy(mut self, strategy: impl Into<String>) -> Self {
        self.strategy = Some(strategy.into());
        self
    }

    pub fn require_role(mut self, role: impl Into<String>) -> Self {
        self.required_roles.insert(role.into());
        self
    }

    pub fn require_roles<I, R>(mut self, roles: I) -> Self
    where
        I: IntoIterator<Item = R>,
        R: Into<String>,
    {
        self.required_roles
            .extend(roles.into_iter().map(Into::into));
        self
    }

    pub fn require_scope(mut self, scope: impl Into<String>) -> Self {
        self.required_scopes.insert(scope.into());
        self
    }

    pub fn require_scopes<I, S>(mut self, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_scopes
            .extend(scopes.into_iter().map(Into::into));
        self
    }
}

impl Guard for AuthGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let strategy = self.strategy.clone();
        let required_roles = self.required_roles.clone();
        let required_scopes = self.required_scopes.clone();
        Box::pin(async move {
            if context
                .metadata_as::<bool>(AUTH_PUBLIC_METADATA)?
                .unwrap_or(false)
            {
                return Ok(true);
            }

            let auth = context
                .request
                .get::<AuthService>()
                .map_err(|error| match error {
                    BootError::MissingProvider(_) => BootError::Internal(
                        "AuthService provider is not registered; import AuthModule".to_string(),
                    ),
                    error => error,
                })?;
            let strategy_name = match context.metadata_as::<String>(AUTH_STRATEGY_METADATA)? {
                Some(strategy) => strategy,
                None => strategy.unwrap_or(auth.default_strategy()?),
            };
            let Some(principal) = auth.authenticate(&strategy_name, context.clone()).await? else {
                return Err(BootError::Unauthorized(
                    "missing authentication credentials".to_string(),
                ));
            };

            assert_required_roles(&principal, required_roles.iter())?;
            assert_required_scopes(&principal, required_scopes.iter())?;
            if let Some(roles) = context.metadata_as::<Vec<String>>(AUTH_ROLES_METADATA)? {
                assert_required_roles(&principal, roles.iter())?;
            }
            if let Some(scopes) = context.metadata_as::<Vec<String>>(AUTH_SCOPES_METADATA)? {
                assert_required_scopes(&principal, scopes.iter())?;
            }

            context.request.set_auth_principal(principal)?;
            Ok(true)
        })
    }
}

/// Module that registers auth strategies and exports [`AuthService`].
#[derive(Clone)]
pub struct AuthModule {
    name: &'static str,
    service: Arc<AuthService>,
    default_strategy: String,
    imports: Vec<Arc<dyn Module>>,
    providers: Vec<ProviderDefinition>,
    strategies: Vec<AuthStrategyDefinition>,
    global: bool,
}

impl fmt::Debug for AuthModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthModule")
            .field("name", &self.name)
            .field("default_strategy", &self.default_strategy)
            .field("strategies", &self.strategies.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl AuthModule {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            service: Arc::new(AuthService::default()),
            default_strategy: "bearer".to_string(),
            imports: Vec::new(),
            providers: Vec::new(),
            strategies: Vec::new(),
            global: false,
        }
    }

    pub fn default_strategy(mut self, strategy: impl Into<String>) -> Self {
        self.default_strategy = strategy.into();
        self
    }

    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.imports.push(Arc::new(module));
        self
    }

    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.imports.push(module);
        self
    }

    pub fn provider(mut self, provider: ProviderDefinition) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn strategy<S>(mut self, name: impl Into<String>, strategy: S) -> Self
    where
        S: AuthStrategy,
    {
        self.strategies
            .push(AuthStrategyDefinition::new(name, strategy));
        self
    }

    pub fn strategy_definition(mut self, definition: AuthStrategyDefinition) -> Self {
        self.strategies.push(definition);
        self
    }

    pub fn bearer<V>(self, verifier: V) -> Self
    where
        V: BearerTokenVerifier,
    {
        self.bearer_named("bearer", verifier)
            .default_strategy("bearer")
    }

    pub fn bearer_named<V>(mut self, name: impl Into<String>, verifier: V) -> Self
    where
        V: BearerTokenVerifier,
    {
        self.strategies
            .push(AuthStrategyDefinition::bearer(name, verifier));
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for AuthModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        self.imports.clone()
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let mut providers = vec![ProviderDefinition::from_arc(Arc::clone(&self.service))];
        providers.extend(self.providers.clone());
        Ok(providers)
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<AuthService>()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.service
            .set_default_strategy(self.default_strategy.clone())?;
        for strategy in &self.strategies {
            self.service
                .register_strategy_definition(strategy.clone())?;
        }
        Ok(())
    }
}

fn assert_required_roles<'a, I>(principal: &AuthPrincipal, roles: I) -> Result<()>
where
    I: IntoIterator<Item = &'a String>,
{
    for role in roles {
        if !principal.has_role(role) {
            return Err(BootError::Forbidden(format!(
                "missing required role: {role}"
            )));
        }
    }
    Ok(())
}

fn assert_required_scopes<'a, I>(principal: &AuthPrincipal, scopes: I) -> Result<()>
where
    I: IntoIterator<Item = &'a String>,
{
    for scope in scopes {
        if !principal.has_scope(scope) {
            return Err(BootError::Forbidden(format!(
                "missing required scope: {scope}"
            )));
        }
    }
    Ok(())
}
