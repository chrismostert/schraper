use std::{thread, time::Duration};

use anyhow::Result;
use schraper::job::{
    JobKind, Jobs,
};

#[tokio::main]
async fn main() -> Result<()> {
    let poll_rate = Duration::from_secs(1);
    let mut jobs = Jobs::init().await?.add(JobKind::Movies, Duration::from_secs(3600));

    loop {
        jobs.poll().await?;
        thread::sleep(poll_rate);
    }
}
