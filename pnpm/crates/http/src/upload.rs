use crate::{Error, Result, error::Kind};
use futures_util::future::{Either, select};
use serde_json::{Value, json};

const CHUNK_SIZE: usize = 65_536;

pub(crate) async fn send(mut message: Value, body: Option<&[u8]>) -> Result<Value> {
    let Some(body) = body.filter(|body| body.len() > CHUNK_SIZE) else {
        message["body"] = json!(body);
        return pnpm_wasm_host::request(&message).map_err(Error::host)?.await.map_err(Error::host);
    };
    let upload = Upload::create().await?;
    message["bodyHandle"] = json!(upload.handle);
    let response = pnpm_wasm_host::request(&message).map_err(Error::host)?;
    match select(response, Box::pin(upload.write(body))).await {
        Either::Left((response, _writing)) => response.map_err(Error::host),
        Either::Right((written, response)) => {
            written?;
            response.await.map_err(Error::host)
        }
    }
}

struct Upload {
    handle: u32,
}

impl Upload {
    async fn create() -> Result<Self> {
        let result = pnpm_wasm_host::request(&json!({ "operation": "upload.create" }))
            .map_err(Error::host)?
            .await
            .map_err(Error::host)?;
        let handle = result["handle"]
            .as_u64()
            .and_then(|handle| u32::try_from(handle).ok())
            .ok_or_else(|| Error::new(Kind::Decode, "Invalid HTTP upload handle"))?;
        Ok(Self { handle })
    }
    async fn write(&self, bytes: &[u8]) -> Result<()> {
        for bytes in bytes.chunks(CHUNK_SIZE) {
            pnpm_wasm_host::request(
                &json!({ "operation": "upload.write", "handle": self.handle, "bytes": bytes }),
            )
            .map_err(Error::host)?
            .await
            .map_err(Error::host)?;
        }
        pnpm_wasm_host::request(&json!({ "operation": "upload.end", "handle": self.handle }))
            .map_err(Error::host)?
            .await
            .map_err(Error::host)?;
        Ok(())
    }
}

impl Drop for Upload {
    fn drop(&mut self) {
        if let Err(error) = pnpm_wasm_host::close_resource(self.handle) {
            eprintln!("Failed to close HTTP upload stream: {error}");
        }
    }
}
