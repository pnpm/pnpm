use crate::local::Connection;
use crate::TransactionBehavior;
use crate::{params::Params, Result};
use std::ops::Deref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropBehavior {
    Rollback,
    Commit,
    Ignore,
    Panic,
}

pub struct Transaction {
    conn: Connection,
    drop_behavior: DropBehavior,
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if self.conn.is_autocommit() {
            return;
        }
        match self.drop_behavior {
            DropBehavior::Rollback => {
                self.do_rollback().unwrap();
            }
            DropBehavior::Commit => {
                self.do_commit().unwrap();
            }
            DropBehavior::Ignore => {}
            DropBehavior::Panic => {
                if !std::thread::panicking() {
                    panic!("Transaction dropped without being committed or rolled back");
                }
            }
        }
    }
}

impl Transaction {
    pub fn drop_behavior(&self) -> DropBehavior {
        self.drop_behavior
    }

    pub fn set_drop_behavior(&mut self, drop_behavior: DropBehavior) {
        self.drop_behavior = drop_behavior
    }

    /// Begin a new transaction in the given mode.
    pub(crate) fn begin(conn: Connection, tx_behavior: TransactionBehavior) -> Result<Self> {
        let begin_stmt = match tx_behavior {
            TransactionBehavior::Deferred => "BEGIN DEFERRED",
            TransactionBehavior::Immediate => "BEGIN IMMEDIATE",
            TransactionBehavior::Exclusive => "BEGIN EXCLUSIVE",
            TransactionBehavior::ReadOnly => "BEGIN READONLY",
        };
        let _ = conn.execute(begin_stmt, Params::None)?;
        Ok(Self {
            conn,
            drop_behavior: DropBehavior::Rollback,
        })
    }

    /// Commit the transaction.
    pub fn commit(self) -> Result<()> {
        self.do_commit()
    }

    fn do_commit(&self) -> Result<()> {
        let _ = self.conn.execute("COMMIT", Params::None)?;
        Ok(())
    }

    /// Rollback the transaction.
    pub fn rollback(self) -> Result<()> {
        self.do_rollback()
    }

    fn do_rollback(&self) -> Result<()> {
        let _ = self.conn.execute("ROLLBACK", Params::None)?;
        Ok(())
    }
}

impl Deref for Transaction {
    type Target = Connection;

    #[inline]
    fn deref(&self) -> &Connection {
        &self.conn
    }
}
