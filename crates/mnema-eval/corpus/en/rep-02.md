# Post-incident review: Lanterna outage of 12 August

This review covers the loss of service on 12 August and the work that followed it. Three engineers who handled the incident wrote it, and the whole of Northgate read it together a week later. Nothing here assigns blame; the failure had at least three contributing causes and no single author.

The queue node stopped acknowledging writes at 09:14, and every ingest worker blocked behind it within a minute. Health checks kept reporting that node as fine, because they test a socket rather than a write, so the first real signal anyone saw was a customer mail. Twelve minutes passed between the first blocked worker and the first page.

Lanterna stayed unreachable for forty-seven minutes, from 09:14 to 10:01 on 12 August. The clock starts at the first blocked write and stops when the export endpoint served a report again. Sixty-one customer accounts were active in that window, and thirty-eight of them opened a ticket.

Recovery meant draining the stuck node by hand, promoting its replica, and restarting every worker in a fixed order. That order matters, and it was written down nowhere; one engineer reconstructed it from memory, which is why the last fifteen minutes were slower than they had to be. A second engineer spent that stretch answering the support queue by hand, which nobody had planned for and everybody afterwards agreed was the right call.

Ninety-four percent of the queued jobs replayed successfully once the node came back, and the remainder were rebuilt by hand. No customer data was lost, though four scheduled reports were delivered late enough to miss their own deadline. Finance put the direct cost of the day, including the credit offered to affected accounts, at eleven thousand euros.

Nine follow-up actions came out of this review, ranging from a real write-based health check to a written recovery order kept beside the runbook. Each has an owner and a date, and none of them is a promise to be more careful. The queue migration that removes this single point of failure was already planned; the review moved it forward by one quarter rather than inventing it.
