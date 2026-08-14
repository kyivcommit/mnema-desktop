# On-call runbook for Quarry pages

This runbook covers the pages that reach the on-call engineer directly. Aleks Fenner keeps it current, and every edit needs a ticket in the platform queue. If you are paged for something not described here, treat it as a new failure mode and write it up afterwards. Read the whole page once before your first shift rather than for the first time at four in the morning.

A health check that fails twice in a row pages whoever holds the rota, and the checks themselves are thirty seconds apart. Acknowledge inside five minutes so the alert does not escalate to the second tier. The page names the host, so start by opening its console rather than the dashboard. Two pages for the same host inside an hour count as one incident, so do not open a second ticket for the repeat.

Draining a node takes sixty seconds, and the service will not accept a shutdown until that window closes. Look at the spool directory before you drain, because a full spool will hold the drain at zero for the whole window. A hard stop during ingest leaves half written batches that somebody has to reconcile by hand the next morning.

Promotion of a standby is never automatic, and nobody may start one without paging Rina Coldwell first. The cluster tolerates a single missing member for hours, so there is rarely any hurry. Two members promoted at once will split the roster, and recovering from that has taken most of a working day before.

A standby node that has just been rebuilt soaks for twenty minutes before it accepts live traffic. Watch its queue depth through the soak, because a figure climbing past five thousand means the router is already sending it real work. Send the drain command over the admin socket if that happens, and let the node settle before you try again.

Handing the rota over in the middle of an incident needs a spoken handover, never a line dropped into the channel. Say which host you touched, what you have already ruled out, and what you were about to try next. The incoming engineer owns the page from that moment, and the one going off stays reachable for another hour.

Every page ends with a short write-up in the platform queue, even the ones that cleared themselves. Say what you saw, what you did, and what you wanted to exist and did not. Aleks Fenner reads them on Mondays and folds anything repeated into this page.
