use anyhow::{anyhow, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub trait Closeable: Send + Sync {
    fn close(&self);
}

pub struct BaseCloseable {
    list: Vec<Box<dyn Closeable>>,
}

impl BaseCloseable {
    pub fn new() -> Self {
        Self { list: Vec::new() }
    }

    pub fn add(&mut self, c: Box<dyn Closeable>) {
        self.list.push(c);
    }

    pub fn close(&mut self) {
        while let Some(c) = self.list.pop() {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                c.close();
            }));
        }
    }
}

pub struct PlatformGuard;

impl PlatformGuard {
    pub fn ensure(enabled_any: bool) -> Result<()> {
        if !enabled_any {
            return Err(anyhow!(
                "No platform enabled! Please enable them in /config/application.toml"
            ));
        }
        Ok(())
    }
}

pub struct StartPlatforms;
