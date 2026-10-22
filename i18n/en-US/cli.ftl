# dismpro-cli messages (TD-0002 §4). Source locale: en-US (ADR-0005).

cli-usage = Usage: dismpro-cli [--target <target>] [--json] <command> [options]
cli-help =
    Commands:
      targets list                     List servicing targets
      packages list                    List packages of the target
      packages remove <identity>...    Plan or apply package removal
      features list                    List optional features
      features enable|disable <name>...  Plan or apply feature changes
      health check|scan                Check component store health (read-only)
      health restore [--source <path>] Plan or apply a component store repair
      reboot-reasons                   Explain why a reboot is pending
      journal show|undo <file>         Inspect or undo a journaled job
    Options:
      --target <online|D:|path>        Target to service (default: online)
      --json                           Machine-readable output (schemas/*.v1.json)
      --dry-run                        Show the plan only
      --yes                            Apply without asking (reversible changes only)
      --allow-irreversible             With --yes: also apply permanent changes
      --color <auto|always|never>      Colored output
      --version                        Print the version
cli-version = dismpro-cli { $version }
cli-nothing-to-do = Nothing to do.
cli-plan-summary = { $count } changes · { $bytes } · { $irreversible } permanent
cli-confirm-apply = Apply { $count } changes? [y/N]
cli-confirm-irreversible = { $irreversible } changes are permanent. Type { $count } to apply all { $count } changes:
cli-confirmation-required = Not applied: pass --yes to apply (and --allow-irreversible for permanent changes).
cli-applied = Applied { $applied } of { $count } changes.
cli-job-partial = { $failed } changes failed; the rest were applied and can be undone.
cli-cancelled = Cancelled after { $applied } changes.
cli-error = error: { $message }
cli-error-usage = { $message }. Run dismpro-cli --help.
cli-error-elevation = This command needs an elevated prompt (Run as administrator).
cli-error-target-unknown = Unknown target { $target }. Known targets: { $known }
cli-error-plan-stale = Things changed since the plan was made. Run the command again.
cli-error-unsupported = This command needs Windows.
cli-error-not-implemented = { $what } isn't implemented yet.

col-target = Target
col-kind = Kind
col-path = Path
col-package = Package
col-feature = Feature
col-state = State
col-release = Release
col-installed = Installed
col-action = Action
col-object = Object
col-size = Size
col-undo = Undo
col-reason = Reason

badge-undoable = Undoable
badge-undoable-until = Undoable until { $until }
badge-permanent = Permanent
badge-reboot = Reboot

state-not-present = Not present
state-uninstall-pending = Uninstall pending
state-staged = Staged
state-removed = Removed
state-installed = Installed
state-install-pending = Install pending
state-superseded = Superseded
state-partially-installed = Partially installed
state-unknown = Unknown ({ $value })

change-file_quarantine = Move to quarantine
change-file_delete = Delete
change-pending_quarantine = Quarantine at restart
change-reg_set = Set registry value
change-reg_delete = Delete registry entry
change-service_start = Change service start
change-service_delete = Delete service
change-task_state = Change scheduled task
change-task_delete = Delete scheduled task
change-startup_entry = Change startup entry
change-firewall_rule_delete = Delete firewall rule
change-feature_state = Change feature
change-appx_remove = Remove app
change-package_add = Add package
change-package_remove = Remove package
change-driver_add = Add driver
change-driver_remove = Remove driver
change-msi_registration_delete = Delete installer registration
change-native = Run servicing operation
change-external_program = Run program
change-file_restore = Restore from quarantine

unit-bytes = { $value } B
unit-kb = { $value } KB
unit-mb = { $value } MB
unit-gb = { $value } GB
unit-tb = { $value } TB
fmt-decimal-separator = .
