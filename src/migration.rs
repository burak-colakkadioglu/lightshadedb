use crate::options::{env_bool, env_u64};
use crate::table::FieldType;
use crate::{Error, MigrationError, Result};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableSummary {
    pub name: String,
    pub fields: Vec<FieldSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSummary {
    pub name: String,
    pub ty: FieldType,
    pub primary_key: bool,
    pub has_default: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SchemaSummary {
    pub fingerprint: u64,
    pub tables: Vec<TableSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MigrationStep {
    AddTable {
        table: String,
    },
    RemoveTable {
        table: String,
    },
    RenameTable {
        from: String,
        to: String,
    },
    AddField {
        table: String,
        field: String,
        ty: FieldType,
        default: Option<String>,
    },
    RemoveField {
        table: String,
        field: String,
    },
    RenameField {
        table: String,
        from: String,
        to: String,
    },
    ConvertField {
        table: String,
        field: String,
        from: FieldType,
        to: FieldType,
    },
}

impl MigrationStep {
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::RemoveTable { .. } | Self::RemoveField { .. })
    }
}

#[derive(Clone, Debug, Default)]
pub struct MigrationPlan {
    pub from: SchemaSummary,
    pub to: SchemaSummary,
    pub steps: Vec<MigrationStep>,
    pub additions: usize,
    pub removals: usize,
    pub destructive: bool,
}

impl MigrationPlan {
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn has_destructive_steps(&self) -> bool {
        self.destructive
    }
}

#[derive(Clone, Debug)]
pub struct MigrationOptions {
    pub allow_destructive: bool,
    pub backup: bool,
    pub backup_path: Option<PathBuf>,
}

impl Default for MigrationOptions {
    fn default() -> Self {
        Self {
            allow_destructive: false,
            backup: true,
            backup_path: None,
        }
    }
}

impl MigrationOptions {
    pub fn from_env() -> Result<Self> {
        let mut options = Self::default();
        if let Some(value) = env_bool("LIGHTSHADE_MIGRATION_BACKUP")? {
            options.backup = value;
        }
        if let Some(value) = env_bool("LIGHTSHADE_ALLOW_DESTRUCTIVE_MIGRATION")? {
            options.allow_destructive = value;
        }
        let _ = env_u64;
        Ok(options)
    }

    pub fn check(&self, plan: &MigrationPlan) -> Result<()> {
        if plan.has_destructive_steps() && !self.allow_destructive {
            return Err(Error::Migration(MigrationError::DestructiveNotAllowed));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct MigrationReport {
    pub plan: MigrationPlan,
    pub from_lsn: u64,
    pub to_lsn: u64,
    pub rows: u64,
    pub backup: Option<PathBuf>,
    pub duration: Duration,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct TableHint {
    pub compiled_name: String,
    pub stored_name: Option<String>,
    pub fields: Vec<FieldHint>,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct FieldHint {
    pub compiled_name: String,
    pub stored_name: Option<String>,
    pub convert_from: Option<FieldType>,
}

#[doc(hidden)]
pub fn build_plan(
    stored: SchemaSummary,
    compiled: SchemaSummary,
    hints: &[TableHint],
) -> Result<MigrationPlan> {
    let mut steps = Vec::new();
    let mut matched_stored = vec![false; stored.tables.len()];

    for table in &compiled.tables {
        let hint = hints.iter().find(|hint| hint.compiled_name == table.name);
        let stored_name = hint
            .and_then(|hint| hint.stored_name.clone())
            .unwrap_or_else(|| table.name.clone());
        let Some(stored_index) = stored
            .tables
            .iter()
            .position(|candidate| candidate.name == stored_name)
        else {
            steps.push(MigrationStep::AddTable {
                table: table.name.clone(),
            });
            continue;
        };
        matched_stored[stored_index] = true;
        let stored_table = &stored.tables[stored_index];
        if stored_table.name != table.name {
            steps.push(MigrationStep::RenameTable {
                from: stored_table.name.clone(),
                to: table.name.clone(),
            });
        }

        let stored_primary = stored_table.fields.iter().find(|field| field.primary_key);
        let compiled_primary = table.fields.iter().find(|field| field.primary_key);
        let stored_primary_name = hint
            .and_then(|hint| {
                hint.fields.iter().find(|field| {
                    Some(&field.compiled_name) == compiled_primary.map(|field| &field.name)
                })
            })
            .and_then(|field| field.stored_name.clone())
            .or_else(|| compiled_primary.map(|field| field.name.clone()));
        if let (Some(stored_primary), Some(compiled_primary)) = (stored_primary, compiled_primary) {
            if stored_primary.ty != compiled_primary.ty
                || stored_primary_name.as_deref() != Some(stored_primary.name.as_str())
            {
                return Err(Error::Migration(MigrationError::PrimaryKeyChanged {
                    table: table.name.clone(),
                }));
            }
        }

        let mut matched_fields = vec![false; stored_table.fields.len()];
        for field in &table.fields {
            let field_hint = hint.and_then(|hint| {
                hint.fields
                    .iter()
                    .find(|candidate| candidate.compiled_name == field.name)
            });
            let field_stored_name = field_hint
                .and_then(|hint| hint.stored_name.clone())
                .unwrap_or_else(|| field.name.clone());
            let Some(stored_field_index) = stored_table
                .fields
                .iter()
                .position(|candidate| candidate.name == field_stored_name)
            else {
                if !field.has_default && !field.nullable {
                    return Err(Error::Migration(MigrationError::MissingDefault {
                        table: table.name.clone(),
                        field: field.name.clone(),
                    }));
                }
                steps.push(MigrationStep::AddField {
                    table: table.name.clone(),
                    field: field.name.clone(),
                    ty: field.ty,
                    default: None,
                });
                continue;
            };
            matched_fields[stored_field_index] = true;
            let stored_field = &stored_table.fields[stored_field_index];
            if stored_field.nullable && !field.nullable {
                return Err(Error::Migration(MigrationError::NullabilityChanged {
                    table: table.name.clone(),
                    field: field.name.clone(),
                }));
            }
            if stored_field.ty != field.ty {
                let accepts = field_hint
                    .and_then(|hint| hint.convert_from)
                    .is_some_and(|from| from == stored_field.ty);
                if !accepts {
                    return Err(Error::Migration(MigrationError::ConversionRequired {
                        table: table.name.clone(),
                        field: field.name.clone(),
                        from: stored_field.ty,
                        to: field.ty,
                    }));
                }
                steps.push(MigrationStep::ConvertField {
                    table: table.name.clone(),
                    field: field.name.clone(),
                    from: stored_field.ty,
                    to: field.ty,
                });
            } else if stored_field.name != field.name {
                steps.push(MigrationStep::RenameField {
                    table: table.name.clone(),
                    from: stored_field.name.clone(),
                    to: field.name.clone(),
                });
            }
        }
        for (index, stored_field) in stored_table.fields.iter().enumerate() {
            if !matched_fields[index] {
                steps.push(MigrationStep::RemoveField {
                    table: table.name.clone(),
                    field: stored_field.name.clone(),
                });
            }
        }
    }

    for (index, stored_table) in stored.tables.iter().enumerate() {
        if !matched_stored[index] {
            steps.push(MigrationStep::RemoveTable {
                table: stored_table.name.clone(),
            });
        }
    }

    let additions = steps
        .iter()
        .filter(|step| {
            matches!(
                step,
                MigrationStep::AddTable { .. } | MigrationStep::AddField { .. }
            )
        })
        .count();
    let removals = steps.iter().filter(|step| step.is_destructive()).count();
    let destructive = removals > 0;
    Ok(MigrationPlan {
        from: stored,
        to: compiled,
        steps,
        additions,
        removals,
        destructive,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, ty: FieldType, primary_key: bool) -> FieldSummary {
        FieldSummary {
            name: name.into(),
            ty,
            primary_key,
            has_default: true,
            nullable: false,
        }
    }

    #[test]
    fn destructive_plans_need_opt_in() {
        let plan = MigrationPlan {
            steps: vec![MigrationStep::RemoveTable {
                table: "old".into(),
            }],
            destructive: true,
            ..MigrationPlan::default()
        };
        assert!(plan.has_destructive_steps());
        assert!(MigrationOptions::default().check(&plan).is_err());
        let allow = MigrationOptions {
            allow_destructive: true,
            ..MigrationOptions::default()
        };
        assert!(allow.check(&plan).is_ok());
    }

    #[test]
    fn plans_add_remove_and_rename() {
        let stored = SchemaSummary {
            fingerprint: 1,
            tables: vec![TableSummary {
                name: "old_notes".into(),
                fields: vec![
                    field("id", FieldType::I64, true),
                    field("body", FieldType::Text, false),
                ],
            }],
        };
        let compiled = SchemaSummary {
            fingerprint: 2,
            tables: vec![
                TableSummary {
                    name: "notes".into(),
                    fields: vec![
                        field("id", FieldType::I64, true),
                        field("text", FieldType::Text, false),
                    ],
                },
                TableSummary {
                    name: "tags".into(),
                    fields: vec![
                        field("id", FieldType::I64, true),
                        field("name", FieldType::Text, false),
                    ],
                },
            ],
        };
        let hints = [TableHint {
            compiled_name: "notes".into(),
            stored_name: Some("old_notes".into()),
            fields: vec![FieldHint {
                compiled_name: "text".into(),
                stored_name: Some("body".into()),
                convert_from: None,
            }],
        }];
        let plan = build_plan(stored, compiled, &hints).unwrap();
        assert!(plan.steps.contains(&MigrationStep::RenameTable {
            from: "old_notes".into(),
            to: "notes".into()
        }));
        assert!(plan.steps.contains(&MigrationStep::RenameField {
            table: "notes".into(),
            from: "body".into(),
            to: "text".into()
        }));
        assert!(plan.steps.contains(&MigrationStep::AddTable {
            table: "tags".into()
        }));
        assert_eq!(plan.additions, 1);
        assert!(!plan.destructive);
    }
}
