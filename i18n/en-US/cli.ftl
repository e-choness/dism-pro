# dismpro-cli messages (TD-0002 §4). Source locale: en-US (ADR-0005).
# The interim loader has no selectors yet (roadmap 3.4), so counts are worded as
# "Label: { $count }" rather than "{ $count } things".

cli-usage = Usage: dismpro-cli [--target <target>] [--json] <command> [options]
cli-help =
    Commands:
      targets list                     List servicing targets
      packages list                    List packages of the target
      packages remove <identity>...    Plan or apply package removal
      features list                    List optional features
      features enable|disable <name>...  Plan or apply feature changes
      health check|scan                Check component store health (read-only)
      health restore                   Plan or apply a component store repair
      reboot-reasons                   Explain why a reboot is pending
      journal list                     List journaled jobs on the target
      journal show <job-id|file>       Show what a job did
      journal undo <job-id|file>       Plan or apply the undo of a job
      purge                            Remove all DISM Pro data from the target volume
    Options:
      --target <online|D:|path|image:dir>  Target to service (default: online)
      --json                           JSON Lines output, one document per line (schemas/*.v1.json)
      --dry-run                        Show the plan only
      --yes, -y                        Apply without asking (undoable changes only)
      --allow-irreversible             With --yes: also apply permanent changes
      --source <path>                  Repair source for health restore (repeatable)
      --limit-access                   Health restore: don't use Windows Update
      --version                        Print the version
cli-version = dismpro-cli { $version }
cli-nothing-to-do = Nothing to do.
cli-plan-summary = Changes: { $count } · Size: { $bytes } · Permanent: { $irreversible }
cli-confirm-apply = Apply these changes ({ $count })? [y/N]
cli-confirm-irreversible = Permanent changes: { $irreversible } of { $count }. To apply all of them, type { $count }:
cli-confirmation-required = Not applied: pass --yes to apply (and --allow-irreversible for permanent changes).
cli-applied = Applied: { $applied } of { $count }.
cli-job-partial = Failed: { $failed }.
cli-job-failed-change = { $action } { $object }: { $code }
cli-cancelled = Cancelled. Changes applied before that stay journaled and can be undone.
cli-reboot-required = Restart Windows to finish these changes.
cli-journal-file = Journal: { $path }
cli-progress = { $percent }%
cli-progress-step = [{ $step }/{ $total }] { $percent }%
cli-error = error: { $message }
cli-error-failed = { $message }
cli-error-usage = { $message }. Run dismpro-cli --help.
cli-error-elevation = This command needs an elevated prompt (Run as administrator).
cli-error-target-unknown = Unknown target { $target }. Known targets: { $known }
cli-error-plan-stale = Things changed since the plan was made. Run the command again.
cli-error-unsupported = This command needs Windows.
cli-error-not-implemented = { $what } isn't implemented yet.
cli-error-cancelled = Cancelled.

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
col-job = Job
col-created = Created
col-operation = Operation
col-status = Status
col-applied = Applied

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
change-feature_enable = Enable feature
change-feature_disable = Disable feature
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
fmt-utc = { $date } { $time } UTC

# Operation titles (OpDescriptor.title_key)
op-features-enable = Enable Windows features
op-features-disable = Disable Windows features
op-packages-remove = Remove packages
op-journal-undo = Undo a previous job
op-health-restore = Repair the component store
op-purge = Remove all DISM Pro data from this volume

# Journal (TD-0001 §5.4)
job-status-completed = Completed
job-status-cancelled = Cancelled
job-status-aborted = Stopped by an error
job-status-interrupted = Interrupted
job-undone-by = Undone by { $job }
journal-none = No journaled jobs on this target.
journal-already-undone = Job { $job } was already undone by job { $by }. To redo it, undo { $by }.
journal-other-target = Job { $job } belongs to a different Windows installation than the target.
journal-other-build = Job { $job } was made on build { $then }; the target is now build { $now }, so undo is disabled.

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
health-restore-wu-note = The repair may download files from Windows Update. Pass --limit-access to use only --source.
reboot-none = No reboot is pending.
reboot-reason-cbs = Windows servicing finished installing or removing components and needs a restart.
reboot-reason-cbs-packages = Windows servicing has package changes waiting for a restart.
reboot-reason-pending-xml = Component changes are queued to finish at the next restart.
reboot-reason-wu = Windows Update installed updates that need a restart.
reboot-reason-file-rename = Files waiting to be replaced or deleted at restart: { $count }.
reboot-reason-computer-rename = The computer was renamed; the new name applies after a restart.
reboot-reason-update-exe = An update installer asked for a restart.
severity-info = Info
severity-warning = Warning
severity-error = Error
