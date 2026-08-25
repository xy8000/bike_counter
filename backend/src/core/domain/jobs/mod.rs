//! Business domain module for generic jobs.
//!
//! - Model: [`job::Job`] + [`job::JobStatus`].
//! - Driven port: [`repository_port::JobRepository`] (implemented by
//!   `PostgresJobRepository`).
//! - Driving port: [`service_port::JobServicePort`] (implemented by
//!   `JobService`).

pub mod job;
pub mod repository_port;
pub mod scheduled_job_port;
pub mod service_port;
