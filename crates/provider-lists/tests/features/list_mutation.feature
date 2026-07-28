Feature: Changing a list only ever writes what actually changed

  A napplet granted the "lists" capability may add to and remove from the
  user's NIP-51 lists. The runtime decides the resulting membership before
  anything is written, and the napplet is told what happened only once the
  change is durable.

  Background:
    Given a napplet with an open lists session
    And the account's follow list already contains "alice"

  Scenario: Adding someone new proposes exactly one write
    When the napplet adds "bob" to the follow list
    Then a write is proposed
    And the proposed list is exactly "alice, bob"
    And the napplet has not been told anything yet

  Scenario: The napplet learns the outcome only when the write is durable
    When the napplet adds "bob" to the follow list
    And the write becomes durable
    Then the napplet is told 1 entry was added
    And the napplet received exactly 1 result

  Scenario: A write that never lands reports nothing added
    When the napplet adds "bob" to the follow list
    And the write fails
    Then the napplet is told 0 entries were added
    And the napplet is told the change did not succeed

  Scenario: Adding someone already followed writes nothing
    When the napplet adds "alice" to the follow list
    Then no write is proposed
    And the napplet is told 0 entries were added
    And the napplet is told 1 entry was skipped

  Scenario: Removing someone present proposes the list without them
    When the napplet removes "alice" from the follow list
    Then a write is proposed
    And the proposed list is exactly ""

  Scenario: Removing someone absent writes nothing
    When the napplet removes "bob" from the follow list
    Then no write is proposed
    And the napplet is told 0 entries were removed

  Scenario: A list this runtime does not service is refused by name
    When the napplet adds "bob" to list kind 1
    Then no write is proposed
    And the napplet is told "this runtime does not service list kind 1"
    And the list was never read

  Scenario: A change is refused outright when no account is connected
    Given no account is connected
    When the napplet adds "bob" to the follow list
    Then no write is proposed
    And the napplet is told "no account is connected, so there is no list to change"
    And the list was never read
