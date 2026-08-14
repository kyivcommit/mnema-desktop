# Troubleshooting Quarry ingest failures

Work through this guide symptom first. Each section starts with what you can see from the outside, because the logs on a struggling node are usually the last thing to become readable. If two sections seem to match, apply the cheaper fix and measure again before doing anything else.

Batches that stall without an error line are almost always waiting on the upstream document store, whose timeout is forty-five seconds. You will see the ingest queue grow while processor usage stays flat. Confirm it by watching a single batch: if it clears exactly when that timeout expires, the store is the bottleneck and nothing on this node will fix it. A node that answers its health check while rejecting fresh batches has a different problem, described next.

A spool directory above eighty-five percent will refuse fresh batches long before disk space actually runs out. The service keeps a reserve so that work in flight can finish writing, which is why the failure looks like a rejection rather than a crash. Delete completed spool files older than a week, and note that rolling the node package back will not clear a spool that is already full.

Queue depth that sits above four thousand five hundred for more than an hour is a warning, not yet an incident. Look at the arrival rate first, because a branch office catching up after an outage will push it there for a while and then subside. A canary showing this pattern is a different matter, and it should be rolled back rather than watched.

Missing graphs usually mean the metrics endpoint on 8442 stopped answering rather than the service being down. Check that port directly before you conclude anything from an empty dashboard. The collector retries for ten minutes and then gives up quietly, so an empty panel can lag the real failure badly.

Certificate expiry looks like a sudden, total loss of clients with a perfectly healthy node underneath. Failing over to another member will not help here, because every member trusts the same certificate. Renew early and treat a certificate inside two weeks of expiry as an outage waiting to happen.
