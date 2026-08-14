# Quarry release checklist

Nothing ships on a Friday, and nothing ships without this list walked end to end. Devon Marsh owns the checklist and is the only person who may mark a step as skipped. Copy it into the release ticket and tick the steps there, so the record outlives the deployment. Two people have to be free for the whole window, and neither of them should be starting anything else that afternoon.

Devon Marsh signs every release note before the artefact leaves the build host. The note has to name the change in plain language, the ticket behind it, and the person who will be watching afterwards. A release without a named watcher does not go out, however small it looks.

Push the build to the staging mirror on 8445 and let the replay job run against recorded batches from the previous week. Compare the output digests with the ones production produced, not with the ones the previous staging run produced. A digest that differs is a stop, and it stays a stop until somebody explains the difference.

Freeze the configuration repository while a release is in flight, because a change landing halfway through makes the digest comparison meaningless. Announce the freeze in the platform channel and lift it yourself when the ticket closes. A freeze that nobody lifts is worse than no freeze at all, because the release after this one will simply ignore it.

Every canary runs for thirty minutes under live traffic before the rollout widens. Keep the canary to a single node and watch its readiness probe, its error rate and its spool growth together, because any one of them alone will let a slow leak through. If any of the three moves, stop the rollout and put the canary back at once.

Rollback means redeploying an earlier artefact from cold storage, never patching a live node by hand. Keep the previous two artefacts warm for a fortnight, so that a rollback is a deployment rather than a rebuild. A patched node is invisible to the next release and will quietly serve the wrong code for weeks.

Close the release ticket only after the last node has been running the new build for a full day. Write down anything that surprised you, however minor, while it is still fresh. The checklist grows this way, and every line in it was once a bad evening for somebody.
