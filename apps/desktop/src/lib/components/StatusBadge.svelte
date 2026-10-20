<script lang="ts">
  import type { SessionStatus } from '../types';

  let { status }: { status: SessionStatus } = $props();

  const labels: Record<string, string> = {
    connecting: 'Connecting',
    authenticating: 'Authenticating',
    ready: 'Connected',
    reconnecting: 'Reconnecting',
    closed: 'Disconnected',
    failed: 'Error',
  };
</script>

<span class="badge {status.state}" title={status.state}>
  <span class="dot" aria-hidden="true"></span>
  {labels[status.state] ?? status.state}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.1rem 0.5rem;
    border-radius: 999px;
    border: 1px solid var(--termru-border);
    font-size: 0.72rem;
    color: var(--termru-muted);
    white-space: nowrap;
  }

  .dot {
    width: 0.45rem;
    height: 0.45rem;
    border-radius: 50%;
    background: currentColor;
  }

  .ready {
    color: var(--termru-success);
  }

  .connecting,
  .authenticating,
  .reconnecting {
    color: var(--termru-accent);
  }

  .failed {
    color: var(--termru-danger);
  }
</style>
