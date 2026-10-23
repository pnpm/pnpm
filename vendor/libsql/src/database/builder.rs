cfg_core! {
    use crate::EncryptionConfig;
}

use super::DbType;
use crate::{Database, Result};

#[cfg(any(feature = "remote", feature = "sync"))]
pub use crate::database::EncryptionContext;

/// A builder for [`Database`]. This struct can be used to build
/// all variants of [`Database`]. These variants include:
///
/// - `new_local`/`Local` which means a `Database` that is just a local libsql database
///     it does no networking and does not connect to any remote database.
/// - `new_remote_replica`/`RemoteReplica` creates an embedded replica database that will be able
///     to sync from the remote url and delegate writes to the remote primary.
/// - `new_synced_database`/`SyncedDatabase` creates a database that can be written offline and
///     synced to a remote server.
/// - `new_local_replica`/`LocalReplica` creates an embedded replica similar to the remote version
///     except you must use `Database::sync_frames` to sync with the remote. This version also
///     includes the ability to delegate writes to a remote primary.
/// - `new_remote`/`Remote` creates a database that does not create anything locally but will
///     instead run all queries on the remote database. This is essentially the pure HTTP api.
///
/// # Note
///
/// Embedded replicas require a clean database (no database file) or a previously synced database or else it will
/// throw an error to prevent any misuse. To work around this error a user can delete the database
/// and let it resync and create the wal_index metadata file.
pub struct Builder<T = ()> {
    inner: T,
}

impl Builder<()> {
    cfg_core! {
        /// Create a new local database.
        pub fn new_local(path: impl AsRef<std::path::Path>) -> Builder<Local> {
            Builder {
                inner: Local {
                    path: path.as_ref().to_path_buf(),
                    flags: crate::OpenFlags::default(),
                    encryption_config: None,
                    skip_safety_assert: false,
                },
            }
        }
    }

    cfg_replication! {
        /// Create a new remote embedded replica.
        pub fn new_remote_replica(
            path: impl AsRef<std::path::Path>,
            url: String,
            auth_token: String,
        ) -> Builder<RemoteReplica> {
            Builder {
                inner: RemoteReplica {
                    path: path.as_ref().to_path_buf(),
                    remote: Remote {
                        url,
                        auth_token,
                        connector: None,
                        version: None,
                        namespace: None,
                        #[cfg(any(feature = "remote", feature = "sync"))]
                        remote_encryption: None,
                    },
                    encryption_config: None,
                    read_your_writes: true,
                    sync_interval: None,
                    http_request_callback: None,
                    skip_safety_assert: false,
                    #[cfg(feature = "sync")]
                    sync_protocol: Default::default(),
                    #[cfg(feature = "sync")]
                    remote_encryption: None
                },
            }
        }

        /// Create a new local replica.
        pub fn new_local_replica(path: impl AsRef<std::path::Path>) -> Builder<LocalReplica> {
            Builder {
                inner: LocalReplica {
                    path: path.as_ref().to_path_buf(),
                    flags: crate::OpenFlags::default(),
                    remote: None,
                    encryption_config: None,
                    http_request_callback: None
                },
            }
        }
    }

    cfg_sync! {
        /// Create a database that can be written offline and synced to a remote server.
        pub fn new_synced_database(
            path: impl AsRef<std::path::Path>,
            url: String,
            auth_token: String,
        ) -> Builder<SyncedDatabase> {
            Builder {
                inner: SyncedDatabase {
                    path: path.as_ref().to_path_buf(),
                    flags: crate::OpenFlags::default(),
                    remote: Remote {
                        url,
                        auth_token,
                        connector: None,
                        version: None,
                        namespace: None,
                        remote_encryption: None,
                    },
                    connector: None,
                    read_your_writes: true,
                    remote_writes: false,
                    push_batch_size: 0,
                    sync_interval: None,
                    remote_encryption: None,
                },
            }
        }
    }

    cfg_remote! {
        /// Create a new remote database.
        pub fn new_remote(url: String, auth_token: String) -> Builder<Remote> {
            Builder {
                inner: Remote {
                    url,
                    auth_token,
                    connector: None,
                    version: None,
                    namespace: None,
                    remote_encryption: None,
                },
            }
        }
    }
}

cfg_replication_or_remote_or_sync! {
    /// Remote configuration type used in [`Builder`].
    pub struct Remote {
        url: String,
        auth_token: String,
        connector: Option<crate::util::ConnectorService>,
        version: Option<String>,
        namespace: Option<String>,
        #[cfg(any(feature = "remote", feature = "sync"))]
        remote_encryption: Option<EncryptionContext>,
    }
}

cfg_core! {
    /// Local database configuration type in [`Builder`].
    pub struct Local {
        path: std::path::PathBuf,
        flags: crate::OpenFlags,
        encryption_config: Option<EncryptionConfig>,
        skip_safety_assert: bool,
    }

    impl Builder<Local> {
        /// Set [`OpenFlags`] for this database.
        pub fn flags(mut self, flags: crate::OpenFlags) -> Builder<Local> {
            self.inner.flags = flags;
            self
        }

        /// Set an encryption config that will encrypt the local database.
        pub fn encryption_config(
            mut self,
            encryption_config: EncryptionConfig,
        ) -> Builder<Local> {
            self.inner.encryption_config = Some(encryption_config);
            self
        }

        /// Skip the saftey assert used to ensure that sqlite3 is configured correctly for the way
        /// that libsql uses the ffi code. By default, libsql will try to use the SERIALIZED
        /// threadsafe mode for sqlite3. This allows us to implement Send/Sync for all the types to
        /// allow them to move between threads safely. Due to the fact that sqlite3 has a global
        /// config this may conflict with other sqlite3 connections in the same process.
        ///
        /// Using this setting is very UNSAFE and you are expected to use the libsql in adherence
        /// with the sqlite3 threadsafe rules or else you WILL create undefined behavior. Use at
        /// your own risk.
        pub unsafe fn skip_safety_assert(mut self, skip: bool) -> Builder<Local> {
            self.inner.skip_safety_assert = skip;
            self
        }

        /// Build the local database.
        pub async fn build(self) -> Result<Database> {
            let db = if self.inner.path == std::path::Path::new(":memory:") {
                let db = if !self.inner.skip_safety_assert {
                    crate::local::Database::open(":memory:", crate::OpenFlags::default())?
                } else {
                    unsafe { crate::local::Database::open_raw(":memory:", crate::OpenFlags::default())? }
                };

                Database {
                    db_type: DbType::Memory { db } ,
                    max_write_replication_index: Default::default(),
                }
            } else {
                let path = self
                    .inner
                    .path
                    .to_str()
                    .ok_or(crate::Error::InvalidUTF8Path)?
                    .to_owned();

                Database {
                    db_type: DbType::File {
                        path,
                        flags: self.inner.flags,
                        encryption_config: self.inner.encryption_config,
                        skip_safety_assert: self.inner.skip_safety_assert
                    },
                    max_write_replication_index: Default::default(),
                }
            };

            Ok(db)
        }
    }
}

cfg_replication! {
    /// Remote replica configuration type in [`Builder`].
    pub struct RemoteReplica {
        path: std::path::PathBuf,
        remote: Remote,
        encryption_config: Option<EncryptionConfig>,
        read_your_writes: bool,
        sync_interval: Option<std::time::Duration>,
        http_request_callback: Option<crate::util::HttpRequestCallback>,
        skip_safety_assert: bool,
        #[cfg(feature = "sync")]
        sync_protocol: super::SyncProtocol,
        #[cfg(feature = "sync")]
        remote_encryption: Option<EncryptionContext>,
    }

    /// Local replica configuration type in [`Builder`].
    pub struct LocalReplica {
        path: std::path::PathBuf,
        flags: crate::OpenFlags,
        remote: Option<Remote>,
        encryption_config: Option<EncryptionConfig>,
        http_request_callback: Option<crate::util::HttpRequestCallback>,
    }

    impl Builder<RemoteReplica> {
        /// Provide a custom http connector that will be used to create http connections.
        pub fn connector<C>(mut self, connector: C) -> Builder<RemoteReplica>
        where
            C: tower::Service<http::Uri> + Send + Clone + Sync + 'static,
            C::Response: crate::util::Socket,
            C::Future: Send + 'static,
            C::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        {
            self.inner.remote = self.inner.remote.connector(connector);
            self
        }

        /// Set an encryption key that will encrypt the local database.
        pub fn encryption_config(
            mut self,
            encryption_config: EncryptionConfig,
        ) -> Builder<RemoteReplica> {
            self.inner.encryption_config = Some(encryption_config.into());
            self
        }

        /// Set whether you want writes to be visible locally before the write query returns. This
        /// means that you will be able to read your own writes if this is set to `true`.
        ///
        /// # Default
        ///
        /// This defaults to `true`.
        pub fn read_your_writes(mut self, read_your_writes: bool) -> Builder<RemoteReplica> {
            self.inner.read_your_writes = read_your_writes;
            self
        }

        /// Set the duration at which the replicator will automatically call `sync` in the
        /// background. The sync will continue for the duration that the resulted `Database`
        /// type is alive for, once it is dropped the background task will get dropped and stop.
        pub fn sync_interval(mut self, duration: std::time::Duration) -> Builder<RemoteReplica> {
            self.inner.sync_interval = Some(duration);
            self
        }

        /// Set the duration at which the replicator will automatically call `sync` in the
        /// background. The sync will continue for the duration that the resulted `Database`
        /// type is alive for, once it is dropped the background task will get dropped and stop.
        #[cfg(feature = "sync")]
        pub fn sync_protocol(mut self, protocol: super::SyncProtocol) -> Builder<RemoteReplica> {
            self.inner.sync_protocol = protocol;
            self
        }

        /// Set the encryption context if the database is encrypted in remote server.
        #[cfg(feature = "sync")]
        pub fn remote_encryption(mut self, encryption_context: EncryptionContext) -> Builder<RemoteReplica> {
            self.inner.remote_encryption = Some(encryption_context);
            self
        }

        pub fn http_request_callback<F>(mut self, f: F) -> Builder<RemoteReplica>
        where
            F: Fn(&mut http::Request<()>) + Send + Sync + 'static
        {
            self.inner.http_request_callback = Some(std::sync::Arc::new(f));
            self

        }

        /// Set the namespace that will be communicated to remote replica in the http header.
        pub fn namespace(mut self, namespace: impl Into<String>) -> Builder<RemoteReplica>
        {
            self.inner.remote.namespace = Some(namespace.into());
            self
        }

        #[doc(hidden)]
        pub fn version(mut self, version: String) -> Builder<RemoteReplica> {
            self.inner.remote = self.inner.remote.version(version);
            self
        }

        /// Skip the safety assert used to ensure that sqlite3 is configured correctly for the way
        /// that libsql uses the ffi code. By default, libsql will try to use the SERIALIZED
        /// threadsafe mode for sqlite3. This allows us to implement Send/Sync for all the types to
        /// allow them to move between threads safely. Due to the fact that sqlite3 has a global
        /// config this may conflict with other sqlite3 connections in the same process.
        ///
        /// Using this setting is very UNSAFE and you are expected to use the libsql in adherence
        /// with the sqlite3 threadsafe rules or else you WILL create undefined behavior. Use at
        /// your own risk.
        pub unsafe fn skip_safety_assert(mut self, skip: bool) -> Builder<RemoteReplica> {
            self.inner.skip_safety_assert = skip;
            self
        }

        /// Build the remote embedded replica database.
        pub async fn build(self) -> Result<Database> {
            let RemoteReplica {
                path,
                remote:
                    Remote {
                        url,
                        auth_token,
                        connector,
                        version,
                        namespace,
                        ..
                    },
                encryption_config,
                read_your_writes,
                sync_interval,
                http_request_callback,
                skip_safety_assert,
                #[cfg(feature = "sync")]
                sync_protocol,
                #[cfg(feature = "sync")]
                remote_encryption,
            } = self.inner;

            let connector = if let Some(connector) = connector {
                connector
            } else {
                let https = super::connector()?;
                use tower::ServiceExt;

                let svc = https
                    .map_err(|e| e.into())
                    .map_response(|s| Box::new(s) as Box<dyn crate::util::Socket>);

                crate::util::ConnectorService::new(svc)
            };

            #[cfg(feature = "sync")]
            {
                use super::SyncProtocol;
                match sync_protocol {
                    p @ (SyncProtocol::Auto | SyncProtocol::V2) => {
                        tracing::trace!("Probing for sync protocol version for {}", url);
                        let client = hyper::client::Client::builder()
                            .build::<_, hyper::Body>(connector.clone());

                        let prefix = if url.starts_with("libsql://") {
                            url.replacen("libsql://", "https://", 1)
                        } else {
                            url.to_string()
                        };
                        let req = http::Request::get(format!("{prefix}/info"))
                            .header("Authorization", format!("Bearer {}", auth_token));

                        let req = if let Some(ref remote_encryption) = remote_encryption {
                            req.header("x-turso-encryption-key", remote_encryption.key.as_string())
                        } else {
                            req
                        };
                        let req = req.body(hyper::Body::empty()).unwrap();

                        let res = client
                            .request(req)
                            .await
                            .map_err(|err| crate::Error::Sync(err.into()))?;

                        tracing::trace!("Probe for sync protocol version for {} returned status {}", url, res.status());

                        if res.status() == http::StatusCode::UNAUTHORIZED {
                            return Err(crate::Error::Sync("Unauthorized".into()));
                        }

                        if matches!(p, SyncProtocol::V2) {
                            if !res.status().is_success() {
                                let status = res.status();
                                let body_bytes = hyper::body::to_bytes(res.into_body())
                                    .await
                                    .map_err(|err| crate::Error::Sync(err.into()))?;
                                let error_message = String::from_utf8_lossy(&body_bytes);
                                return Err(crate::Error::Sync(format!("HTTP error {}: {}", status, error_message).into()));
                            }
                        }

                        if res.status().is_success() {
                            tracing::trace!("Using sync protocol v2 for {}", url);
                            let mut builder = Builder::new_synced_database(path, url, auth_token)
                                .connector(connector)
                                .remote_writes(true)
                                .read_your_writes(read_your_writes);

                            if let Some(encryption) = remote_encryption {
                                builder = builder.remote_encryption(encryption);
                            }

                            let builder = if let Some(sync_interval) = sync_interval {
                                builder.sync_interval(sync_interval)
                            } else {
                                builder
                            };

                            return builder.build().await;
                        }
                        tracing::trace!("Using sync protocol v1 for {} based on probe results", url);
                    }
                    SyncProtocol::V1 => {
                        tracing::trace!("Using sync protocol v1 for {}", url);
                    }
                }
            }

            let path = path.to_str().ok_or(crate::Error::InvalidUTF8Path)?.to_owned();

            let db = if !skip_safety_assert {
                crate::local::Database::open_http_sync_internal(
                    connector,
                    path,
                    url,
                    auth_token,
                    version,
                    read_your_writes,
                    encryption_config.clone(),
                    sync_interval,
                    http_request_callback,
                    namespace,
                )
                .await?
            } else {
                // SAFETY: this can only be enabled via the unsafe config function
                // `skip_safety_assert`.
                unsafe  {
                    crate::local::Database::open_http_sync_internal2(
                        connector,
                        path,
                        url,
                        auth_token,
                        version,
                        read_your_writes,
                        encryption_config.clone(),
                        sync_interval,
                        http_request_callback,
                        namespace,
                    )
                    .await?
                }

            };


            Ok(Database {
                db_type: DbType::Sync {
                    db,
                    encryption_config,
                },
                max_write_replication_index: Default::default(),
            })
        }
    }

    impl Builder<LocalReplica> {
        /// Set [`OpenFlags`] for this database.
        pub fn flags(mut self, flags: crate::OpenFlags) -> Builder<LocalReplica> {
            self.inner.flags = flags;
            self
        }

        pub fn http_request_callback<F>(mut self, f: F) -> Builder<LocalReplica>
        where
            F: Fn(&mut http::Request<()>) + Send + Sync + 'static
        {
            self.inner.http_request_callback = Some(std::sync::Arc::new(f));
            self

        }

        /// Build the local embedded replica database.
        pub async fn build(self) -> Result<Database> {
            let LocalReplica {
                path,
                flags,
                remote,
                encryption_config,
                http_request_callback
            } = self.inner;

            let path = path.to_str().ok_or(crate::Error::InvalidUTF8Path)?.to_owned();

            let db = if let Some(Remote {
                url,
                auth_token,
                connector,
                version,
                namespace,
                ..
            }) = remote
            {
                let connector = if let Some(connector) = connector {
                    connector
                } else {
                    let https = super::connector()?;
                    use tower::ServiceExt;

                    let svc = https
                        .map_err(|e| e.into())
                        .map_response(|s| Box::new(s) as Box<dyn crate::util::Socket>);

                    crate::util::ConnectorService::new(svc)
                };

                crate::local::Database::open_local_sync_remote_writes(
                    connector,
                    path,
                    url,
                    auth_token,
                    version,
                    flags,
                    encryption_config.clone(),
                    http_request_callback,
                    namespace,
                )
                .await?
            } else {
                crate::local::Database::open_local_sync(path, flags, encryption_config.clone()).await?
            };

            Ok(Database {
                db_type: DbType::Sync { db, encryption_config },
                max_write_replication_index: Default::default(),
            })
        }
    }
}

cfg_sync! {
    /// Remote replica configuration type in [`Builder`].
    pub struct SyncedDatabase {
        path: std::path::PathBuf,
        flags: crate::OpenFlags,
        remote: Remote,
        connector: Option<crate::util::ConnectorService>,
        remote_writes: bool,
        read_your_writes: bool,
        push_batch_size: u32,
        sync_interval: Option<std::time::Duration>,
        remote_encryption: Option<EncryptionContext>,
    }

    impl Builder<SyncedDatabase> {
        #[doc(hidden)]
        pub fn version(mut self, version: String) -> Builder<SyncedDatabase> {
            self.inner.remote = self.inner.remote.version(version);
            self
        }

        pub fn read_your_writes(mut self, v: bool) -> Builder<SyncedDatabase> {
            self.inner.read_your_writes = v;
            self
        }

        pub fn remote_writes(mut self, v: bool) -> Builder<SyncedDatabase> {
            self.inner.remote_writes = v;
            self
        }

        pub fn set_push_batch_size(mut self, v: u32) -> Builder<SyncedDatabase> {
            self.inner.push_batch_size = v;
            self
        }

        /// Set the duration at which the replicator will automatically call `sync` in the
        /// background. The sync will continue for the duration that the resulted `Database`
        /// type is alive for, once it is dropped the background task will get dropped and stop.
        pub fn sync_interval(mut self, duration: std::time::Duration) -> Builder<SyncedDatabase> {
            self.inner.sync_interval = Some(duration);
            self
        }

         /// Set the encryption context if the database is encrypted in remote server.
        pub fn remote_encryption(mut self, encryption_context: EncryptionContext) -> Builder<SyncedDatabase> {
            self.inner.remote_encryption = Some(encryption_context);
            self
        }

        /// Provide a custom http connector that will be used to create http connections.
        pub fn connector<C>(mut self, connector: C) -> Builder<SyncedDatabase>
        where
            C: tower::Service<http::Uri> + Send + Clone + Sync + 'static,
            C::Response: crate::util::Socket,
            C::Future: Send + 'static,
            C::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        {
            self.inner.connector = Some(wrap_connector(connector));
            self
        }

        /// Build a connection to a local database that can be synced to remote server.
        pub async fn build(self) -> Result<Database> {
            use tracing::Instrument as _;

            let SyncedDatabase {
                path,
                flags,
                remote:
                    Remote {
                        url,
                        auth_token,
                        connector: _,
                        version: _,
                        namespace: _,
                        ..
                    },
                connector,
                remote_writes,
                read_your_writes,
                push_batch_size,
                sync_interval,
                remote_encryption,
            } = self.inner;

            let path = path.to_str().ok_or(crate::Error::InvalidUTF8Path)?.to_owned();

            let https = if let Some(connector) = connector {
                connector
            } else {
                wrap_connector(super::connector()?)
            };
            use tower::ServiceExt;

            let svc = https
                .map_err(|e| e.into())
                .map_response(|s| Box::new(s) as Box<dyn crate::util::Socket>);

            let connector = crate::util::ConnectorService::new(svc);

            let db = crate::local::Database::open_local_with_offline_writes(
                connector.clone(),
                path,
                flags,
                url.clone(),
                auth_token.clone(),
                remote_encryption.clone(),
            )
            .await?;

            if push_batch_size > 0 {
                db.sync_ctx.as_ref().unwrap().lock().await.set_push_batch_size(push_batch_size);
            }

            let mut bg_abort: Option<std::sync::Arc<crate::sync::DropAbort>> = None;


            if let Some(sync_interval) = sync_interval {
                let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

                let sync_span = tracing::debug_span!("sync_interval");
                let _enter = sync_span.enter();

                let sync_ctx = db.sync_ctx.as_ref().unwrap().clone();
                {
                    let mut ctx = sync_ctx.lock().await;
                    crate::sync::bootstrap_db(&mut ctx).await?;
                    tracing::debug!("finished bootstrap with sync interval");
                }

                // db.connect creates a local db file, so it is important that we always call
                // `bootstrap_db` (for synced dbs) before calling connect. Otherwise, the sync
                // protocol skips calling `export` endpoint causing slowdown in initial bootstrap.
                let conn = db.connect()?;

                tokio::spawn(
                    async move {
                        let mut interval = tokio::time::interval(sync_interval);

                        loop {
                            tokio::select! {
                                _ = &mut cancel_rx => break,
                                _ = interval.tick() => {
                                    tracing::debug!("trying to sync");

                                    let mut ctx = sync_ctx.lock().await;

                                    let result = if remote_writes {
                                        crate::sync::try_pull(&mut ctx, &conn).await
                                    } else {
                                        crate::sync::sync_offline(&mut ctx, &conn).await
                                    };

                                    if let Err(e) = result {
                                        tracing::error!("Error syncing database: {}", e);
                                    }
                                }
                            }
                        }
                    }
                    .instrument(tracing::debug_span!("sync interval thread")),
                );

                bg_abort.replace(std::sync::Arc::new(crate::sync::DropAbort(Some(cancel_tx))));
            }

            Ok(Database {
                db_type: DbType::Offline {
                    db,
                    remote_writes,
                    read_your_writes,
                    url,
                    auth_token,
                    connector,
                    _bg_abort: bg_abort,
                    #[cfg(feature = "sync")]
                    remote_encryption,
                },
                max_write_replication_index: Default::default(),
            })
        }
    }
}

cfg_remote! {
    impl Builder<Remote> {
        /// Provide a custom http connector that will be used to create http connections.
        pub fn connector<C>(mut self, connector: C) -> Builder<Remote>
        where
            C: tower::Service<http::Uri> + Send + Clone + Sync + 'static,
            C::Response: crate::util::Socket,
            C::Future: Send + 'static,
            C::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        {
            self.inner = self.inner.connector(connector);
            self
        }

        #[doc(hidden)]
        pub fn version(mut self, version: String) -> Builder<Remote> {
            self.inner = self.inner.version(version);
            self
        }

        /// Set the namespace that will be communicated to the remote in the http header.
        pub fn namespace(mut self, namespace: impl Into<String>) -> Builder<Remote>
        {
            self.inner.namespace = Some(namespace.into());
            self
        }

        /// Set the encryption context if the database is encrypted in remote server.
        pub fn remote_encryption(mut self, encryption_context: EncryptionContext) -> Builder<Remote> {
            self.inner.remote_encryption = Some(encryption_context);
            self
        }

        /// Build the remote database client.
        pub async fn build(self) -> Result<Database> {
            let Remote {
                url,
                auth_token,
                connector,
                version,
                namespace,
                remote_encryption,
            } = self.inner;

            let connector = if let Some(connector) = connector {
                connector
            } else {
                let https = super::connector()?;
                use tower::ServiceExt;

                let svc = https
                    .map_err(|e| e.into())
                    .map_response(|s| Box::new(s) as Box<dyn crate::util::Socket>);

                crate::util::ConnectorService::new(svc)
            };

            Ok(Database {
                db_type: DbType::Remote {
                    url,
                    auth_token,
                    connector,
                    version,
                    namespace,
                    remote_encryption
                },
                max_write_replication_index: Default::default(),
            })
        }
    }
}

cfg_replication_or_remote_or_sync! {
    fn wrap_connector<C>(connector: C) -> crate::util::ConnectorService
    where
        C: tower::Service<http::Uri> + Send + Clone + Sync + 'static,
        C::Response: crate::util::Socket,
        C::Future: Send + 'static,
        C::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
    {
        use tower::ServiceExt;

        let svc = connector
            .map_err(|e| e.into())
            .map_response(|s| Box::new(s) as Box<dyn crate::util::Socket>);

        crate::util::ConnectorService::new(svc)
    }

    impl Remote {
        fn connector<C>(mut self, connector: C) -> Remote
        where
            C: tower::Service<http::Uri> + Send + Clone + Sync + 'static,
            C::Response: crate::util::Socket,
            C::Future: Send + 'static,
            C::Error: Into<Box<dyn std::error::Error + Send + Sync>>,
        {
            self.connector = Some(wrap_connector(connector));
            self
        }

        fn version(mut self, version: String) -> Remote {
            self.version = Some(version);
            self
        }
    }
}
