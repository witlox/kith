Feature: Commit windows and transactional changes
  State-changing operations enter a pending state with a configurable
  commit window. Changes must be explicitly committed or they auto-revert.

  Scenario: Local file change with commit
    Given kith shell is running
    And the commit window is set to 10 minutes
    When the agent edits "/etc/nginx/conf.d/api.conf"
    Then the change is applied via overlayfs overlay
    And the change is marked "pending" with a 10-minute window
    And the user is shown the diff
    When the user types "commit"
    Then the overlay is merged to the base filesystem
    And an audit entry records the commit

  Scenario: Commit window expires — auto-rollback
    Given a pending change exists with a 10-minute window
    When 10 minutes pass without a commit
    Then the overlay is discarded and the file reverts
    And an audit entry records the auto-rollback
    And the user is notified "pending change expired — rolled back"

  Scenario: Explicit rollback
    Given a pending change exists
    When the user types "rollback"
    Then the overlay is discarded and the file reverts

  Scenario: Remote change with commit window
    Given "staging-1" is a mesh member
    When the agent calls apply("staging-1", "docker compose up -d")
    Then the change executes on "staging-1" with a commit window
    When the user types "commit"
    Then the change is finalized on "staging-1"

  Scenario: Multiple pending changes committed atomically
    Given pending changes exist for "file-a.py" and "file-b.py"
    When the user types "commit"
    Then both are committed atomically

  Scenario: Local file change on macOS uses copy-based snapshot
    Given kith shell is running on macOS
    And overlayfs is not available
    When the agent edits "/Users/pim/project/config.toml"
    Then the original is copied to ".kith-backup/config.toml"
    And the edit is applied to the original file
    And the change is marked "pending"
    When the user types "rollback"
    Then the backup is restored to the original path
    And the backup is removed

  Scenario: macOS commit removes backup
    Given a pending change on macOS with a copy-based snapshot
    When the user types "commit"
    Then the backup is removed
    And the edited file remains in place
