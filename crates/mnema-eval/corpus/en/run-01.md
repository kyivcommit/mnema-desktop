# Setting up a Quarry ingest node

Quarry is the ingest service that accepts scanned batches from branch offices and writes them into shared storage. This page covers a node built from nothing: a clean machine, no data on it, and no cluster expecting it yet. Work through the sections in order, because each one assumes the previous section finished cleanly.

Quarry listens on port 8443 for ingest traffic, and that is the only port a client ever needs. Install the package from the internal mirror, copy the sample configuration into the node file under /etc/quarry, and edit the two fields marked as required. Start the service once by hand and confirm that it binds before you enable it at boot.

The readiness probe waits ninety seconds before it reports a fresh node as healthy. That delay is deliberate: a node that joins early starts pulling work while its local cache is still cold, and the first few batches then time out. Do not shorten the delay to make a rebuild look faster.

Quarry refuses to install on a node whose data volume is already above eighty percent. Give each machine at least four hundred gigabytes of local disk, on its own volume, separate from the system partition. Spool files are written and deleted constantly, so a shared volume turns every busy hour into a fight for the same disks.

Administrative commands arrive on a separate socket bound to 8444, reachable only from inside the management subnet. Never expose that socket to the office network, even briefly, because it accepts commands that drop batches without asking for confirmation. Firewall rules for both ports live in the platform repository and are applied by configuration management, never by hand.

Register the finished node with the cluster by adding it to the roster file and reloading the router. A node added as a standby carries no traffic until somebody promotes it, so a rebuild can sit there safely for days. Watch the first ten batches land before you walk away, and record the build in the platform ticket queue so the next person can see when this machine was last touched.
