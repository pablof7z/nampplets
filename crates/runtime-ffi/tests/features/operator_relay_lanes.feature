Feature: Operator relay lanes are admitted by the runtime

  Operator relays arrive from the application bundle, so a mistyped relay
  cannot be corrected while the app is running. The runtime judges them --
  every host used to filter them itself, which put the rules in the shell and
  made a bad relay simply disappear.

  A lane degrades rather than taking the runtime down with it. Emptying a lane
  is the exception: routing through no relays while every other signal reads
  healthy is the failure this exists to prevent.

  Scenario: A usable lane opens with nothing refused
    Given the bundle configures indexer relays "wss://indexer.example"
    And the bundle configures app relays "wss://app.example"
    When the runtime opens
    Then the runtime is open
    And no operator relay is refused

  Scenario: An insecure relay is refused by name and the lane still opens
    Given the bundle configures indexer relays "ws://plaintext.example, wss://indexer.example"
    And the bundle configures app relays "wss://app.example"
    When the runtime opens
    Then the runtime is open
    And exactly 1 operator relay is refused
    And an operator relay refusal names "ws://plaintext.example"

  Scenario: A relay carrying credentials is refused by name
    Given the bundle configures indexer relays "wss://indexer.example"
    And the bundle configures app relays "wss://user:secret@app.example, wss://app.example"
    When the runtime opens
    Then the runtime is open
    And an operator relay refusal names "credentials"

  Scenario: A repeated relay is admitted once and the repeat is refused
    Given the bundle configures indexer relays "wss://indexer.example, wss://indexer.example"
    And the bundle configures app relays "wss://app.example"
    When the runtime opens
    Then the runtime is open
    And an operator relay refusal names "already in this lane"

  Scenario: A lane whose every entry is refused stops the runtime opening
    Given the bundle configures indexer relays "ws://one.example, ws://two.example"
    And the bundle configures app relays "wss://app.example"
    When the runtime opens
    Then the runtime refuses to open naming the emptied "indexer" lane

  Scenario: A lane nobody configured is not an emptied lane
    Given the bundle configures indexer relays ""
    And the bundle configures app relays ""
    When the runtime opens
    Then the runtime is open

  Scenario: Refused operator relays outlive the bounded refusal ring
    Given the bundle configures indexer relays "ws://plaintext.example, wss://indexer.example"
    And the bundle configures app relays "wss://app.example"
    When the runtime opens
    Then the durable operator relay refusals name "ws://plaintext.example"
