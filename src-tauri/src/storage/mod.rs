pub mod atomic;
pub mod backup;
pub mod migration;

use once_cell::sync::Lazy;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static DATA_LOCK: Lazy<RwLock<()>> = Lazy::new(|| RwLock::new(()));

pub fn read_lock() -> RwLockReadGuard<'static, ()> {
    DATA_LOCK
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn write_lock() -> RwLockWriteGuard<'static, ()> {
    DATA_LOCK
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::{read_lock, write_lock};
    use std::{sync::mpsc, thread, time::Duration};

    #[test]
    fn exclusive_lock_blocks_ordinary_writer() {
        let exclusive = write_lock();
        let (tx, rx) = mpsc::channel();
        let writer = thread::spawn(move || {
            let _ordinary = read_lock();
            tx.send(()).unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
        drop(exclusive);
        rx.recv_timeout(Duration::from_secs(1)).unwrap();
        writer.join().unwrap();
    }
}
