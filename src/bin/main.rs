use std::{env, thread, time::Duration};

use anyhow::Result;
use dotenvy::dotenv;
use schraper::job::{
    util::{self, Client},
    JobKind, Jobs,
};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<()> {
    let poll_rate = Duration::from_secs(1);
    let mut jobs = Jobs::init().await?.add(JobKind::Movies, Duration::from_secs(3600));

    loop {
        jobs.poll().await?;
        thread::sleep(poll_rate);
    }
}
