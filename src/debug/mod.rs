use tracing_subscriber::EnvFilter;
use tracing_subscriber::EnvFilter;
use std::{
    any::type_name,
    sync::{Arc, Mutex, MutexGuard},
};

use anyhow::{anyhow, Result};

pub type Shared<T> = Arc<T>;
pub type SharedState<T> = Arc<Mutex<T>>;

#[inline]
pub fn shared<T>(value: T) -> Shared<T> {
    Arc::new(value)
}

#[inline]
pub fn shared_state<T>(value: T) -> SharedState<T> {
    Arc::new(Mutex::new(value))
}


// ============================================================================
// Mutex
// ============================================================================

pub trait MutexExt<T> {
    fn lock_or_err(&self) -> Result<MutexGuard<'_, T>>;

    fn lock_recover(&self) -> MutexGuard<'_, T>;
}

impl<T> MutexExt<T> for Mutex<T> {
    #[inline]
    fn lock_or_err(&self) -> Result<MutexGuard<'_, T>> {
        self.lock().map_err(|_| {
            anyhow!(
                "mutex for `{}` is poisoned",
                type_name::<T>()
            )
        })
    }

    #[inline]
    fn lock_recover(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,

            Err(poisoned) => {
                tracing::warn!(
                    mutex_type = %type_name::<T>(),
                    "recovering poisoned mutex"
                );

                poisoned.into_inner()
            }
        }
    }
}


// ============================================================================
// Result helpers
// ============================================================================

#[macro_export]
macro_rules! log_if_err {
    ($result:expr) => {{
        match $result {
            Ok(value) => Some(value),

            Err(error) => {
                tracing::error!(
                    error = ?error,
                    "operation failed"
                );

                None
            }
        }
    }};
}

#[macro_export]
macro_rules! try_or_log {
    ($body:block) => {{
        let result: anyhow::Result<()> = (|| $body)();

        if let Err(error) = result {
            tracing::error!(
                error = ?error,
                "operation failed"
            );
        }
    }};
}
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("taurino=info"))
        )
        .with_target(true)
        .compact()
        .init();
}