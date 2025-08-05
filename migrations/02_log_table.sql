CREATE TABLE joblogs (
    jobname TEXT NOT NULL,
    run_dt TIMESTAMPTZ NOT NULL DEFAULT current_timestamp
);

CREATE INDEX dt_index
ON joblogs(run_dt DESC);