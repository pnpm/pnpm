use crate::{
    connection::Conn,
    params::Params,
    transaction::Tx, Result, TransactionBehavior,
};

use super::connection::SyncedConnection;

pub struct SyncedTx(SyncedConnection);

impl SyncedTx {
    pub(crate) async fn begin(
        conn: SyncedConnection,
        tx_behavior: TransactionBehavior,
    ) -> Result<Self> {
        conn.execute(
            match tx_behavior {
                TransactionBehavior::Deferred => "BEGIN DEFERRED",
                TransactionBehavior::Immediate => "BEGIN IMMEDIATE",
                TransactionBehavior::Exclusive => "BEGIN EXCLUSIVE",
                TransactionBehavior::ReadOnly => "BEGIN READONLY",
            },
            Params::None,
        )
        .await?;
        Ok(Self(conn.clone()))
    }
}

#[async_trait::async_trait]
impl Tx for SyncedTx {
    async fn commit(&mut self) -> Result<()> {
        self.0.execute("COMMIT", Params::None).await?;
        Ok(())
    }

    async fn rollback(&mut self) -> Result<()> {
        self.0.execute("ROLLBACK", Params::None).await?;
        Ok(())
    }
}
