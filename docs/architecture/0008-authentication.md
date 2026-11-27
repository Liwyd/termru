# 0008. Authentication: an ordered chain driven by the server, questions routed to the UI

* Status: Accepted
* Date: 2026-11-27

## Context

[ADR 0002](0002-ssh-transport.md) gave the engine three single-shot methods:
password, key file, agent. A host tried exactly the one it was configured
with, so everything a real server does beyond that failed in ways the user
could not act on:

* servers that disable `PasswordAuthentication` but ask for the password
  through keyboard-interactive (PAM), or ask for a one-time code;
* two-factor setups (`AuthenticationMethods publickey,keyboard-interactive`),
  where the first method *succeeds* and the server still says "not done";
* agents with many keys (the server drops the connection after
  `MaxAuthTries` rejected keys) and agents that decline to sign;
* RSA keys signed with the wrong hash, certificates, encrypted keys whose
  passphrase is not stored, and a server banner nobody saw.

Two things around authentication were also wrong in the app: a host-key
question was broadcast to every window and carried no session, and a full
registry of pending questions was *cleared* (silently denying every waiting
question).

## Decision

### The chain

`ConnectParams::auth` is an ordered list of `AuthChoice`s: `Password`,
`PrivateKey` (with optional identity), `Agent` (with an optional preferred
fingerprint) and `KeyboardInteractive`. The engine (`termru-ssh/src/auth.rs`):

1. sends `none` first — some servers accept it, and every server answers with
   the methods it allows;
2. tries the configured methods in order, **skipping methods the server did not
   list** (offering one costs a `MaxAuthTries` attempt for nothing) and
   refreshing the list after every rejection;
3. treats *partial success* as "this method is done, carry on", and revisits
   methods that were skipped before: with `publickey,keyboard-interactive`
   required, a configuration that lists keyboard-interactive first still
   works, because it becomes allowed once the key was accepted;
4. records a method that cannot be used locally (no key file, no agent, a
   passphrase nobody can supply) and goes on; a question the user **cancels**
   ends the whole authentication;
5. when nothing worked, fails with one error that lists what was tried (and
   why each did not work) and what the server allows — never a credential.
   A chain with a single unusable method keeps that method's own error, so
   the UI can still tell "passphrase needed" from "key rejected".

A server that closes the connection while we authenticate (what `sshd` does
after too many failures) is reported as an *authentication* failure listing
what was tried, not as a lost network: retrying would repeat the attempts —
and the questions.

The desktop derives the chain from the host's single method, on purpose (a
user-ordered list is a data-model change that needs its own synced field):

| host method | chain |
| --- | --- |
| password | password → keyboard-interactive answered with the same password |
| key file | key file (passphrase asked for when needed) → keyboard-interactive |
| agent | agent keys → keyboard-interactive |

Keyboard-interactive answers a **lone, hidden, plain "Password:" prompt**
with the stored password, once per attempt; anything else — a one-time code,
"New password:", two questions — goes to the user, and a wrong stored password
is never sent twice (lock-out counters).

### Agent, keys, certificates

* RSA keys (file and agent) sign with the best hash the server announced
  (`server-sig-algs`: `rsa-sha2-512`, then `-256`, then `ssh-rsa`), not a
  hard-coded one.
* At most **5** agent keys are offered, the key matching a configured identity
  fingerprint first. Five, because `MaxAuthTries` is 6: the sixth failure
  disconnects, and the method after the agent would never run. A key the agent
  cannot sign with is skipped, not fatal.
* A certificate next to the key (`<key>-cert.pub`, as `ssh-keygen -s` writes it)
  is offered first; if the server declines it the plain key follows.
  Certificates held by an agent are used as well.
* Windows: the OpenSSH agent's named pipe (`\\.\pipe\openssh-ssh-agent`),
  then Pageant. This code is compiled only on Windows and was **not run**
  (see `docs/limitations.md`).

### Questions for the user

`AuthPrompt` is the engine's callback for keyboard-interactive rounds and key
passphrases (the host-key counterpart is `HostKeyPrompt`). The desktop answers
both through one mechanism (`state/prompts.rs`):

* a question belongs to **one session** and is emitted to the **windows that
  show it** (the frontend listens per window, `ipc-backend.ts`);
* the connect task registers a pending entry and waits; `…_resolve` answers it;
* the entry never outlives the question: answered, timed out (120 s), dropped
  with the connect task (the session was closed or cancelled), or the session
  ended (`SessionManager` end hook → `cancel_session`, which touches only that
  session's entries). Whenever a question ends without the dialog, a
  `…-closed` event removes the dialog from every window;
* the registry is bounded (64). When full, the **new** question is refused; an
  older one a person may be reading is never evicted (the earlier code cleared
  the whole map);
* a question nobody can answer (no window shows the session) is not asked.

The time a person needs is **not** charged to any deadline: the connect budget
already paused for host-key questions, and the authentication budget now
pauses for authentication questions (`AuthContext::prompt_active`). The
network still is: a server that never answers is cut off.

Answers are `SecretValue`s from the moment the command receives them, are never
logged, and are redacted in `Debug`.

### Passphrases

A key passphrase belongs to its **identity**, not to a host:
`identity:<identity id>/passphrase` in the OS credential store (ADR 0004).
Hosts that share an identity share the entry. An entry that an earlier version
stored per host is still read, and removed when the passphrase is saved under
the identity. When a key is encrypted and no passphrase is stored, the engine
asks ("Passphrase for *identity name*", three tries, "incorrect" flagged on a
retry); the dialog offers to remember it, and the frontend stores it only
after the session reached `ready` — a wrong passphrase never reaches the
credential store. The connect dialog therefore no longer asks key hosts for a
passphrase up front.

### Banner

The server's authentication banner is forwarded as `SessionEvent::Banner`
before `Ready` (also when authentication fails: that is when it matters) and
printed as plain text at the start of the terminal. The engine strips
everything a terminal would interpret — control characters including ESC, the
C1 range, bidirectional overrides — and bounds it to 32 KiB: the text comes
from a server the user has not authenticated yet.

### known_hosts

* A host that is known under other key types but now presents a new one gets
  a warning in the trust question (`other_key_types`); the algorithms known
  for the host are requested first in the handshake, so a server with several
  host keys presents one we can verify instead of its own favourite.
* known_hosts is read and written in `spawn_blocking`, not on an async worker.

## Consequences

* Servers with PAM, 2FA, certificates and big agents work; a failed login
  explains itself.
* The chain is a function of the host's method until Phase 5 adds a synced
  `auth_order`; nothing about the synced data model changed.
* Unsupported: GSSAPI/Kerberos, host-based authentication, FIDO security keys
  (`sk-*`), SSH host certificates, the server-initiated password change
  (`SSH_MSG_USERAUTH_PASSWD_CHANGEREQ`).
* A keyboard-interactive round with several prompts is shown as one dialog;
  the dialog is deliberately plain (Phase 4 restyles it).

## Alternatives considered

* **Try every method blindly, in order**: simpler, but burns `MaxAuthTries`
  on methods the server never offered and cannot do 2FA.
* **A single `AuthChoice` with fallbacks inside**: hides the order from the
  data model; the list keeps Phase 5's `auth_order` a pure data change.
* **Store the passphrase as soon as the user types it**: stores wrong ones.
* **Broadcast questions to all windows**: several dialogs for one answer, and
  a window that does not show the session decides about its trust.
