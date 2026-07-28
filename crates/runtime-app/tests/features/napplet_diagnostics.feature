Feature: The runtime decides what is diagnostic, not the shell

  A sandboxed napplet mirrors its own console output to the host. Deciding
  that such a message is not part of the NAP envelope protocol is a
  protocol-membership judgement, so the runtime makes it. A host that decided
  it locally could be told what to ignore by the very content it is sandboxing.

  Background:
    Given a running napplet session

  Scenario: A console entry becomes a typed fact the host can render
    When the napplet reports "intent payload missing" at level "warn"
    Then the runtime reports one diagnostic at level "warn"
    And the diagnostic is not reported as an unrecognised protocol envelope

  Scenario: A severity the runtime does not recognise is not taken at its word
    When the napplet reports "something happened" at level "catastrophe"
    Then the runtime reports one diagnostic at level "unknown"

  Scenario: A diagnostic that cannot be read still leaves a trace
    When the napplet sends a diagnostic with no message
    Then the runtime reports no diagnostic
    And the runtime records that it could not read the diagnostic

  Scenario: A diagnostic never reaches a provider
    When the napplet reports "hello" at level "log"
    Then no provider was called
