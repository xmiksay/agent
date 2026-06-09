use sea_orm::entity::prelude::*;
use serde::Serialize;

/// A runnable AI model in the catalog. `provider_id` points at the
/// `model_providers` row whose `kind` resolves the agent backend/CLI; `model_id`
/// is the id passed to that CLI (`--model`); `alias` is the human name shown in
/// the UI. The price columns are USD per **1M** tokens. `thinking`/`effort` are
/// optional run settings. At most one row carries `is_default = true` (the global
/// fallback).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "models")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub provider_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub model_id: String,
    #[sea_orm(column_type = "Text")]
    pub alias: String,
    pub input_price: f64,
    pub output_price: f64,
    pub cache_write_price: f64,
    pub cache_read_price: f64,
    pub thinking: bool,
    #[sea_orm(column_type = "Text", nullable)]
    pub effort: Option<String>,
    pub is_default: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
