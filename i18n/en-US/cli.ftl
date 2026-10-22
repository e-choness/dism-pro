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
      purge                            Remove all DISM Pro data from the target volume
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

# Health & Repair (TD-0006 §4)
col-severity = Severity
col-finding = Finding
col-lines = Log lines
health-state-healthy = Healthy: no component store corruption detected.
health-state-repairable = Repairable: corruption found. Run "health restore" to repair it.
health-state-non-repairable = Not repairable: point the repair at a matching install image with --source.
health-finding-source-missing = Windows couldn't find the files it needs to repair itself.
health-finding-download-failed = Downloading repair files from Windows Update failed.
health-finding-policy-blocks-wu = Group Policy stops repairs from using Windows Update.
health-finding-store-corrupt = The component store is corrupted.
health-finding-access-denied = Access was denied while servicing.
health-finding-payload-corrupt = A component's files are damaged.
health-finding-unknown-error = Servicing reported an error.
health-next-source = Point the repair at a matching install image (install.wim) with --source.
health-restore-wu-note = Repair the component store (may download files from Windows Update)
op-health-restore = Repair the component store
reboot-none = No reboot is pending.
reboot-reason-cbs = Windows servicing finished installing or removing components and needs a restart.
reboot-reason-wu = Windows Update installed updates that need a restart.
reboot-reason-file-rename = { $count } files are waiting to be replaced or deleted at restart.
reboot-reason-computer-rename = The computer was renamed; the new name applies after a restart.
reboot-reason-update-exe = An update installer asked for a restart.
severity-info = Info
severity-warning = Warning
severity-error = Error
op-purge = Remove all DISM Pro data from this volume
