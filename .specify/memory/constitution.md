<!--
Sync Impact Report

- Version change: template(placeholders) -> 1.0.0
- Modified principles:
	- (new) Code Quality
	- (new) Testing Standards
	- (new) Security
	- (new) User Privacy
	- (new) Performance & Reliability
- Added sections: Additional Constraints, Development Workflow, Governance
- Removed placeholders: all bracketed template tokens replaced with concrete text
- Templates requiring updates:
	- .specify/templates/plan-template.md ✅ updated
	- .specify/templates/spec-template.md ✅ updated
	- .specify/templates/tasks-template.md ✅ updated
	- .specify/templates/commands/*.md ⚠ pending (no commands/ directory present)
	- .specify/templates/checklist-template.md ⚠ review recommended
-- End Sync Impact Report
-->

# obscura-server Constitution

## Core Principles

### I. Code Quality (NON-NEGOTIABLE)
The codebase MUST be maintainable, well-structured, and readable. Every public API,
module, and service MUST include clear intent documentation, concise examples, and
automated linting rules. Changes that reduce testability, introduce duplication,
or obscure intent are NOT permitted without explicit justification recorded in the
associated issue or PR.

Rationale: High code quality reduces long-term cost, speeds onboarding, and makes
security and correctness guarantees achievable.

### II. Testing Standards (NON-NEGOTIABLE)
Testing is mandatory and MUST follow a test-first approach for all new features
and bugfixes. For production-impacting changes, a developer MUST add failing
tests that demonstrate the intended behavior before implementation (unit,
integration, or contract tests as appropriate). CI gates MUST enforce test
execution and require coverage reports for critical modules (coverage thresholds
defined per-module in `tasks.md` or spec if needed).

Rationale: Tests catch regressions early and provide executable documentation
for expected behavior.

### III. Security (NON-NEGOTIABLE)
Security MUST be prioritized by design. Secrets MUST never be committed to the
repository. All inputs MUST be validated and privilege boundaries enforced. New
dependencies or changes that affect authentication/authorization flows MUST be
reviewed for threat models and include security tests or automated scans.

Rationale: Proactive security practices reduce risk and protect user data and
service integrity.

### IV. User Privacy (NON-NEGOTIABLE)
User data collection, storage, and processing MUST minimize personally
identifiable information (PII). Data retention policies MUST be explicit and
implemented (delete/aggregate where appropriate).

Rationale: Respecting user privacy is a legal and ethical requirement and
reduces liability.

### V. Performance & Reliability (REQUIRED)
Features and services MUST define performance goals (latency, throughput, and
resource budgets) in the plan and spec. Performance regressions MUST be
measured by benchmarks and addressed before merging changes that degrade the
system beyond agreed thresholds. Critical paths MUST include monitoring and
alerting guidance in documentation.

Rationale: Predictable performance and reliability maintain user trust and
enable scalable growth.

## Additional Constraints

Technology and operational constraints for the project:

- Secrets management: Secrets and credentials MUST be stored in an approved
	secret manager; no secrets in source control or CI logs.
- Data handling and retention: The project MUST follow documented retention
	periods; PII MUST be minimized and access controls enforced via RBAC or
	equivalent mechanisms.

These constraints are mandatory and map to the Security and Privacy principles.

## Development Workflow

- Continuous Integration: CI pipelines MUST run linting, tests, and security
	scans. No PR may be merged without passing CI for the target branch.
- Release process: Releases MUST include a changelog entry referencing
	principle impact (e.g., security, privacy, performance) and any required
	migration steps.
- Monitoring & Observability: Production services MUST expose metrics and
	logs sufficient to measure performance goals and investigate incidents.

Quality gates described here implement the Testing, Security, and Performance
principles.

## Governance

Amendments

- Proposal: Amendments MUST be proposed as a pull request titled
	`constitution/amendment: <short summary>` and include a rationale, the
	exact text changes, tests or checks that will enforce the new rule, and a
	migration plan if the change is breaking.
- Review & Approval: Non-breaking clarifications (PATCH) MAY be merged after
	one maintainer approval and passing CI. MINOR changes (new principles or
	material expansions) REQUIRE two maintainer approvals. MAJOR changes
	(removing or redefining existing non-negotiable principles) REQUIRE a
	documented migration plan and approval by a supermajority of maintainers
	(at least two-thirds of maintainers listed in `CODEOWNERS` or project
	maintainers file).
- Emergency Changes: For urgent security fixes, a single trusted maintainer
	MAY merge a temporary amendment but MUST open a follow-up discussion and
	formal PR for retrospective approval within 7 days.

Versioning Policy

- Versions follow semantic versioning for governance text (`MAJOR.MINOR.PATCH`):
	- MAJOR: Backward-incompatible principle removals or redefinitions.
	- MINOR: New principle or substantive expansion of guidance.
	- PATCH: Wording clarifications, typos, or non-functional refinements.

Compliance & Enforcement

- All PRs MUST include a short checklist verifying compliance with applicable
	Constitution principles (e.g., tests added, privacy impact noted).
- The project will run periodic compliance reviews (quarterly) and publish a
	short report of findings and remediation plans.

**Version**: 1.0.0 | **Ratified**: 2025-12-12 | **Last Amended**: 2025-12-11