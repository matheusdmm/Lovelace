use anyhow::{Result, ensure};
use std::{
    process::{Child, ExitStatus},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

/// Owns the current engine so a restart can interrupt it without losing the worker.
#[derive(Default)]
pub struct Control {
    cancelled: AtomicBool,
    child: Mutex<Option<Arc<Mutex<Child>>>>,
}

impl Control {
    pub fn cancel(&self) -> Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Some(child) = self.child.lock().unwrap().as_ref() {
            let mut child = child.lock().unwrap();
            if child.try_wait()?.is_none() {
                child.kill()?;
            }
        }
        Ok(())
    }

    pub fn check(&self) -> Result<()> {
        ensure!(
            !self.cancelled.load(Ordering::SeqCst),
            "Processing interrupted for restart"
        );
        Ok(())
    }
}

pub(crate) struct Engine {
    child: Arc<Mutex<Child>>,
    control: Arc<Control>,
}

impl Engine {
    pub fn new(child: Child, control: Option<Arc<Control>>) -> Result<Self> {
        let control = control.unwrap_or_default();
        let engine = Self {
            child: Arc::new(Mutex::new(child)),
            control,
        };
        *engine.control.child.lock().unwrap() = Some(Arc::clone(&engine.child));
        engine.control.check()?;
        Ok(engine)
    }

    pub fn try_wait(&self) -> Result<Option<ExitStatus>> {
        self.control.check()?;
        Ok(self.child.lock().unwrap().try_wait()?)
    }

    pub fn wait(&self) -> Result<ExitStatus> {
        loop {
            if let Some(status) = self.try_wait()? {
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let mut registered = self.control.child.lock().unwrap();
        let mut child = self.child.lock().unwrap();
        if !matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.kill();
        }
        let _ = child.wait();
        *registered = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_engine_fixture() {
        if std::env::var_os("LOVELACE_TEST_SLEEP").is_some() {
            thread::sleep(Duration::from_secs(30));
        }
    }

    #[test]
    fn cancellation_stops_and_reaps_the_engine_before_returning() -> Result<()> {
        let child = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "job::tests::sleeping_engine_fixture"])
            .env("LOVELACE_TEST_SLEEP", "1")
            .stdout(std::process::Stdio::null())
            .spawn()?;
        let control = Arc::new(Control::default());
        let engine = Engine::new(child, Some(Arc::clone(&control)))?;
        let child = Arc::clone(&engine.child);
        control.cancel()?;
        assert!(engine.wait().is_err());
        drop(engine);
        assert!(child.lock().unwrap().try_wait()?.is_some());
        assert!(control.child.lock().unwrap().is_none());
        Ok(())
    }
}
