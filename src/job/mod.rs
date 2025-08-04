use std::{
    env,
    time::{Duration, Instant},
};

use anyhow::Result;

pub mod matching;
pub mod movies;
pub mod util;

use dotenvy::dotenv;
use movies::MovieFetcher;

use sqlx::PgPool;

trait Runnable {
    async fn run(&self) -> Result<()>;
}

/// Define a job (by name) and it's accompanying 'runner'.
///
/// This 'runner' should be some struct which implements the `Runnable` trait
macro_rules! define_jobs {
    ($(($jobname:ident, $runnable:ident)),+) => {
        pub enum JobKind {
            $($jobname),*
        }

        enum JobRunner {
            $($jobname($runnable)),*
        }

        impl JobRunner {
            fn new(jobkind: JobKind, pool: PgPool) -> JobRunner {
                match jobkind {
                    $(JobKind::$jobname => JobRunner::$jobname($runnable{pool})),*
                }
            }

            async fn run(&self) -> Result<()> {
                match self {
                    $(JobRunner::$jobname(fetcher) => fetcher.run().await),*
                }
            }
        }
    };
}

define_jobs!(
    (Movies, MovieFetcher)
);

struct Job {
    last_ran: Option<Instant>,
    run_interval: Duration,
    job_runner: JobRunner,
}
impl Job {
    fn should_run(&self) -> bool {
        if let Some(time) = self.last_ran {
            return (Instant::now() - time) >= self.run_interval;
        }
        true
    }

    fn new(jobkind: JobKind, interval: Duration, pool: PgPool) -> Self {
        Job {
            last_ran: None,
            run_interval: interval,
            job_runner: JobRunner::new(jobkind, pool),
        }
    }

    async fn run(&mut self) -> Result<()> {
        self.job_runner.run().await?;
        self.last_ran = Some(Instant::now());
        Ok(())
    }
}

pub struct Jobs {
    joblist: Vec<Job>,
    pool: PgPool,
}

impl Jobs {
    /// Initializes the job queue and creates database connection pool
    pub async fn init() -> Result<Self> {
        let _ = dotenv();
        let db_url = env::vars()
            .find(|(k, _)| k == "DATABASE_URL")
            .expect("No database URL supplied")
            .1;
        let pool = PgPool::connect(&db_url).await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Jobs {
            joblist: vec![],
            pool,
        })
    }

    pub fn add(mut self, jobkind: JobKind, interval: Duration) -> Self {
        self.joblist
            .push(Job::new(jobkind, interval, self.pool.clone()));
        self
    }

    /// Polls jobs in the defined order. Executing them in said order.
    pub async fn poll(&mut self) -> Result<()> {
        for job in &mut self.joblist {
            if job.should_run() {
                job.run().await?;
            }
        }
        Ok(())
    }
}
