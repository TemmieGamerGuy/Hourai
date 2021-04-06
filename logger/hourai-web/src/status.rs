use crate::prelude::*;
use crate::AppState;
use actix_web::{get, web};
use serde::Serialize;

#[derive(Serialize)]
struct BotStatus {
    shards: Vec<ShardStatus>,
}

#[derive(Serialize)]
struct ShardStatus {
    shard_id: u16,
    guilds: i64,
    members: i64,
}

#[get("/status")]
async fn bot_status(data: web::Data<AppState>) -> JsonResult<BotStatus> {
    let guilds = hourai_sql::Member::count_guilds()
        .fetch_one(&data.sql)
        .await?
        .0;
    let members = hourai_sql::Member::count_members()
        .fetch_one(&data.sql)
        .await?
        .0;
    Ok(web::Json(BotStatus {
        shards: vec![ShardStatus {
            shard_id: 0,
            guilds,
            members,
        }],
    }))
}

pub fn scoped_config(cfg: &mut web::ServiceConfig) {
    cfg.service(bot_status);
}
