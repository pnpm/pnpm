#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthContext<'a> {
    pub action: AuthAction<'a>,

    pub database_name: Option<&'a str>,

    pub accessor: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthAction<'a> {
    Unknown {
        code: i32,
        arg1: Option<&'a str>,
        arg2: Option<&'a str>,
    },
    CreateIndex {
        index_name: &'a str,
        table_name: &'a str,
    },
    CreateTable {
        table_name: &'a str,
    },
    CreateTempIndex {
        index_name: &'a str,
        table_name: &'a str,
    },
    CreateTempTable {
        table_name: &'a str,
    },
    CreateTempTrigger {
        trigger_name: &'a str,
        table_name: &'a str,
    },
    CreateTempView {
        view_name: &'a str,
    },
    CreateTrigger {
        trigger_name: &'a str,
        table_name: &'a str,
    },
    CreateView {
        view_name: &'a str,
    },
    Delete {
        table_name: &'a str,
    },
    DropIndex {
        index_name: &'a str,
        table_name: &'a str,
    },
    DropTable {
        table_name: &'a str,
    },
    DropTempIndex {
        index_name: &'a str,
        table_name: &'a str,
    },
    DropTempTable {
        table_name: &'a str,
    },
    DropTempTrigger {
        trigger_name: &'a str,
        table_name: &'a str,
    },
    DropTempView {
        view_name: &'a str,
    },
    DropTrigger {
        trigger_name: &'a str,
        table_name: &'a str,
    },
    DropView {
        view_name: &'a str,
    },
    Insert {
        table_name: &'a str,
    },
    Pragma {
        pragma_name: &'a str,
        pragma_value: Option<&'a str>,
    },
    Read {
        table_name: &'a str,
        column_name: &'a str,
    },
    Select,
    Transaction {
        operation: TransactionOperation,
    },
    Update {
        table_name: &'a str,
        column_name: &'a str,
    },
    Attach {
        filename: &'a str,
    },
    Detach {
        database_name: &'a str,
    },
    AlterTable {
        database_name: &'a str,
        table_name: &'a str,
    },
    Reindex {
        index_name: &'a str,
    },
    Analyze {
        table_name: &'a str,
    },
    CreateVtable {
        table_name: &'a str,
        module_name: &'a str,
    },
    DropVtable {
        table_name: &'a str,
        module_name: &'a str,
    },
    Function {
        function_name: &'a str,
    },
    Savepoint {
        operation: TransactionOperation,
        savepoint_name: &'a str,
    },
    Recursive,
}

#[cfg(feature = "core")]
impl<'a> AuthAction<'a> {
    pub(crate) fn from_raw(code: i32, arg1: Option<&'a str>, arg2: Option<&'a str>) -> Self {
        use libsql_sys::ffi;

        match (code, arg1, arg2) {
            (ffi::SQLITE_CREATE_INDEX, Some(index_name), Some(table_name)) => Self::CreateIndex {
                index_name,
                table_name,
            },
            (ffi::SQLITE_CREATE_TABLE, Some(table_name), _) => Self::CreateTable { table_name },
            (ffi::SQLITE_CREATE_TEMP_INDEX, Some(index_name), Some(table_name)) => {
                Self::CreateTempIndex {
                    index_name,
                    table_name,
                }
            }
            (ffi::SQLITE_CREATE_TEMP_TABLE, Some(table_name), _) => {
                Self::CreateTempTable { table_name }
            }
            (ffi::SQLITE_CREATE_TEMP_TRIGGER, Some(trigger_name), Some(table_name)) => {
                Self::CreateTempTrigger {
                    trigger_name,
                    table_name,
                }
            }
            (ffi::SQLITE_CREATE_TEMP_VIEW, Some(view_name), _) => {
                Self::CreateTempView { view_name }
            }
            (ffi::SQLITE_CREATE_TRIGGER, Some(trigger_name), Some(table_name)) => {
                Self::CreateTrigger {
                    trigger_name,
                    table_name,
                }
            }
            (ffi::SQLITE_CREATE_VIEW, Some(view_name), _) => Self::CreateView { view_name },
            (ffi::SQLITE_DELETE, Some(table_name), None) => Self::Delete { table_name },
            (ffi::SQLITE_DROP_INDEX, Some(index_name), Some(table_name)) => Self::DropIndex {
                index_name,
                table_name,
            },
            (ffi::SQLITE_DROP_TABLE, Some(table_name), _) => Self::DropTable { table_name },
            (ffi::SQLITE_DROP_TEMP_INDEX, Some(index_name), Some(table_name)) => {
                Self::DropTempIndex {
                    index_name,
                    table_name,
                }
            }
            (ffi::SQLITE_DROP_TEMP_TABLE, Some(table_name), _) => {
                Self::DropTempTable { table_name }
            }
            (ffi::SQLITE_DROP_TEMP_TRIGGER, Some(trigger_name), Some(table_name)) => {
                Self::DropTempTrigger {
                    trigger_name,
                    table_name,
                }
            }
            (ffi::SQLITE_DROP_TEMP_VIEW, Some(view_name), _) => Self::DropTempView { view_name },
            (ffi::SQLITE_DROP_TRIGGER, Some(trigger_name), Some(table_name)) => Self::DropTrigger {
                trigger_name,
                table_name,
            },
            (ffi::SQLITE_DROP_VIEW, Some(view_name), _) => Self::DropView { view_name },
            (ffi::SQLITE_INSERT, Some(table_name), _) => Self::Insert { table_name },
            (ffi::SQLITE_PRAGMA, Some(pragma_name), pragma_value) => Self::Pragma {
                pragma_name,
                pragma_value,
            },
            (ffi::SQLITE_READ, Some(table_name), Some(column_name)) => Self::Read {
                table_name,
                column_name,
            },
            (ffi::SQLITE_SELECT, ..) => Self::Select,
            (ffi::SQLITE_TRANSACTION, Some(operation_str), _) => Self::Transaction {
                operation: TransactionOperation::from_str(operation_str),
            },
            (ffi::SQLITE_UPDATE, Some(table_name), Some(column_name)) => Self::Update {
                table_name,
                column_name,
            },
            (ffi::SQLITE_ATTACH, Some(filename), _) => Self::Attach { filename },
            (ffi::SQLITE_DETACH, Some(database_name), _) => Self::Detach { database_name },
            (ffi::SQLITE_ALTER_TABLE, Some(database_name), Some(table_name)) => Self::AlterTable {
                database_name,
                table_name,
            },
            (ffi::SQLITE_REINDEX, Some(index_name), _) => Self::Reindex { index_name },
            (ffi::SQLITE_ANALYZE, Some(table_name), _) => Self::Analyze { table_name },
            (ffi::SQLITE_CREATE_VTABLE, Some(table_name), Some(module_name)) => {
                Self::CreateVtable {
                    table_name,
                    module_name,
                }
            }
            (ffi::SQLITE_DROP_VTABLE, Some(table_name), Some(module_name)) => Self::DropVtable {
                table_name,
                module_name,
            },
            (ffi::SQLITE_FUNCTION, _, Some(function_name)) => Self::Function { function_name },
            (ffi::SQLITE_SAVEPOINT, Some(operation_str), Some(savepoint_name)) => Self::Savepoint {
                operation: TransactionOperation::from_str(operation_str),
                savepoint_name,
            },
            (ffi::SQLITE_RECURSIVE, ..) => Self::Recursive,
            (code, arg1, arg2) => Self::Unknown { code, arg1, arg2 },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransactionOperation {
    Unknown,
    Begin,
    Release,
    Rollback,
}

#[cfg(feature = "core")]
impl TransactionOperation {
    fn from_str(op_str: &str) -> Self {
        match op_str {
            "BEGIN" => Self::Begin,
            "RELEASE" => Self::Release,
            "ROLLBACK" => Self::Rollback,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Authorization {
    Allow,
    Ignore,
    Deny,
}
