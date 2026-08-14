# Northgate status summary, week of 8 September

Short version: the quarter closed clean, the August work is nearly finished, and the queue migration is now the only thing on the critical path. Two items need a decision from outside the team, and both are flagged below.

Six of the nine follow-up actions from August are closed, and the rest wait on the queue migration. The three still open are the write-based health check, the recovery order document, and the replica promotion drill, and all three are cheaper to do once rather than twice. Nothing is held up by a shortage of people.

The queue migration has a working prototype behind a flag and moves its first low-volume tenant next week. Two tenants a week after that, largest last, with a fortnight of both queues running in parallel before anything is switched off. The whole move should finish in early November if nothing surprises us.

The Sablewire renewal needs an answer this month. The assessment recommends twelve months rather than twenty-four, and finance has asked whether the migration makes even twelve unnecessary. It does not: the migration will not be finished and proven before the current term ends.

Availability so far this month is back above the published target, with no unplanned interruptions since 12 August. The revised target promised in the quarterly report is drafted but unpublished, because it depends on which queue we are running by January.

Two new engineers started on 1 September and are through their first week of onboarding. Both are working on the migration rather than on-call, deliberately, until they have shipped something small end to end. The export backlog is untouched and stays untouched until November.

Decisions needed from outside the team are the renewal term and the revised availability figure, in that order. Finance owns the first and has the assessment already; the second needs a name from product, because it is a promise to customers rather than an engineering estimate. Everything else on this list can be settled inside Northgate, and will be, without another meeting about it.

