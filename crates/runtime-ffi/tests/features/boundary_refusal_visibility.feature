Feature: A boundary refusal is visible to a consumer that gates on revision

  Native consumers do not poll. They redraw when the runtime's revision moves,
  and skip the frame when it does not. A refusal recorded at the FFI boundary
  that leaves the revision untouched is therefore invisible: the consumer has
  already decided there is nothing new to look at.

  That is the whole failure. The refusal is recorded faithfully, reaches the
  snapshot faithfully, and is never read, because nothing told the reader to
  look again.

  Scenario: A boundary refusal moves the revision a consumer gates on
    Given a consumer has observed the runtime at its current revision
    When the runtime records a boundary refusal
    Then the revision the consumer gates on has moved
    And the refusal is present in the snapshot at that revision

  Scenario: A refusal a consumer never sees is worse than no refusal
    Given a consumer has observed the runtime at its current revision
    When the runtime records a boundary refusal
    Then a consumer redrawing only on a revision change still sees the refusal

  Scenario: Repeated refusals each move the revision
    Given a consumer has observed the runtime at its current revision
    When the runtime records 3 boundary refusals
    Then the revision has moved at least 3 times
    And the snapshot carries 3 refusals
