// Copyright 2023 The CeresDB Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Interpreter for select statement

use async_trait::async_trait;
use common_types::time::TimeRange;
use futures::TryStreamExt;
use generic_error::{BoxError, GenericError};
use logger::debug;
use macros::define_result;
use query_engine::{
    context::ContextRef as QueryContextRef,
    executor::ExecutorRef,
    physical_planner::{PhysicalPlanPtr, PhysicalPlannerRef},
};
use query_frontend::plan::QueryPlan;
use runtime::{Priority, PriorityRuntime};
use snafu::{ResultExt, Snafu};

use crate::{
    context::Context,
    interpreter::{Interpreter, InterpreterPtr, Output, Result as InterpreterResult, Select},
};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to create query context, err:{}", source))]
    CreateQueryContext { source: crate::context::Error },

    #[snafu(display("Failed to execute physical plan, msg:{}, err:{}", msg, source))]
    ExecutePlan { msg: String, source: GenericError },

    #[snafu(display("Failed to spawn task, err:{}", source))]
    Spawn { source: runtime::Error },
}

define_result!(Error);

/// Select interpreter
pub struct SelectInterpreter {
    ctx: Context,
    plan: QueryPlan,
    executor: ExecutorRef,
    physical_planner: PhysicalPlannerRef,
    query_runtime: PriorityRuntime,
}

impl SelectInterpreter {
    pub fn create(
        ctx: Context,
        plan: QueryPlan,
        executor: ExecutorRef,
        physical_planner: PhysicalPlannerRef,
        query_runtime: PriorityRuntime,
    ) -> InterpreterPtr {
        Box::new(Self {
            ctx,
            plan,
            executor,
            physical_planner,
            query_runtime,
        })
    }

    // Currently, we only consider the time range.
    // TODO: consider other factors, such as the number of series, or slow log
    // metrics.
    fn is_expensive_query(time_range: &TimeRange, threshold: u64) -> bool {
        if let Some(v) = time_range
            .exclusive_end()
            .as_i64()
            .checked_sub(time_range.inclusive_start().as_i64())
        {
            v as u64 >= threshold
        } else {
            true
        }
    }
}

#[async_trait]
impl Interpreter for SelectInterpreter {
    async fn execute(self: Box<Self>) -> InterpreterResult<Output> {
        let request_id = self.ctx.request_id();
        let is_expensive_query = if let Some(time_range) = self.plan.extract_time_range() {
            Self::is_expensive_query(&time_range, self.ctx.expensive_query_threshold())
        } else {
            false
        };
        let priority = if is_expensive_query {
            Priority::Lower
        } else {
            Priority::Higher
        };
        let query_ctx = self
            .ctx
            .new_query_context(priority)
            .context(CreateQueryContext)
            .context(Select)?;

        debug!(
            "Interpreter execute select begin, request_id:{}, plan:{:?}, is_expensive:{}",
            request_id, self.plan, is_expensive_query
        );

        // Create physical plan.
        let physical_plan = self
            .physical_planner
            .plan(&query_ctx, self.plan)
            .await
            .box_err()
            .context(ExecutePlan {
                msg: "failed to build physical plan",
            })
            .context(Select)?;

        if is_expensive_query {
            let executor = self.executor;
            return self
                .query_runtime
                .spawn_with_priority(
                    async move {
                        execute_and_collect(query_ctx, executor, physical_plan)
                            .await
                            .context(Select)
                    },
                    Priority::Lower,
                )
                .await
                .context(Spawn)
                .context(Select)?;
        }

        execute_and_collect(query_ctx, self.executor, physical_plan)
            .await
            .context(Select)
    }
}

async fn execute_and_collect(
    query_ctx: QueryContextRef,
    executor: ExecutorRef,
    physical_plan: PhysicalPlanPtr,
) -> Result<Output> {
    let record_batch_stream = executor
        .execute(&query_ctx, physical_plan)
        .await
        .box_err()
        .context(ExecutePlan {
            msg: "failed to execute physical plan",
        })?;

    let record_batches =
        record_batch_stream
            .try_collect()
            .await
            .box_err()
            .context(ExecutePlan {
                msg: "failed to collect execution results",
            })?;

    Ok(Output::Records(record_batches))
}
