mod guild_config;
mod logger;
mod oauth;
mod prelude;
mod status;

use actix_web::{web, App, HttpServer};
use hourai::{config, init};

pub(crate) struct AppState {
    config: hourai::config::HouraiConfig,
    http: awc::Client,
    sql: hourai_sql::SqlPool,
    redis: hourai_redis::RedisPool,
}

pub fn api(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/v1")
            .service(web::scope("/bot").configure(status::scoped_config))
            .service(web::scope("/guilds").configure(guild_config::scoped_config)),
    );
    // OAuth is not versioned
    cfg.service(web::scope("/oauth").configure(oauth::scoped_config));
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let config = config::load_config(config::get_config_path().as_ref());
    init::init(&config);

    let sql = hourai_sql::init(&config).await;
    let redis = hourai_redis::init(&config).await;
    let port = config.web.port;

    HttpServer::new(move || {
        App::new()
            .wrap(logger::TracingLogger)
            .data(AppState {
                config: config.clone(),
                http: awc::Client::new(),
                sql: sql.clone(),
                redis: redis.clone(),
            })
            .service(web::scope("/api").configure(api))
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
}
