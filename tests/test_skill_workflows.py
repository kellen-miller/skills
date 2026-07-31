import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LAVISH_CLI = "LAVISH_AXI_TELEMETRY=0 npx -y lavish-axi"


def read_repo_file(path: str) -> str:
    return ROOT.joinpath(path).read_text(encoding="utf-8")


class GrillPlanBuildPolicyTest(unittest.TestCase):
    def setUp(self):
        self.skill = read_repo_file("grill-plan-build/SKILL.md")

    def test_delegates_each_phase_to_a_bounded_agent(self):
        for phrase in (
            "persistent grill agent",
            "fresh planning agent",
            "persistent implementation agent",
            "fresh closeout reviewer",
            "fresh planning adversarial reviewer",
            "fresh implementation adversarial reviewer",
            "fresh briefing agent",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, self.skill)

    def test_risk_changes_plan_depth_not_review_event_count(self):
        self.assertIn(
            "exactly one `$execplan-improve` attempt",
            self.skill,
        )
        self.assertIn(
            "| Tier | Typical shape | Plan-improvement depth | Planning "
            "adversarial review | Normal closeout review | Implementation "
            "adversarial review |",
            self.skill,
        )
        for tier in ("Standard", "Elevated", "Critical"):
            with self.subTest(tier=tier):
                row = next(
                    line
                    for line in self.skill.splitlines()
                    if line.startswith(f"| {tier} |")
                )
                self.assertEqual(row.count("Exactly one adversarial event"), 2)
                self.assertIn("Exactly one closeout event", row)

        self.assertIn(
            "Risk changes plan-improvement depth and reviewer capability, "
            "not review-event count.",
            self.skill,
        )
        self.assertNotIn("Default to 3", self.skill)
        self.assertNotIn("up to 3 times", self.skill)

    def test_implementation_review_order_is_fixed(self):
        review_section = self.skill.split("### Step 4: Review Agents", 1)[1].split(
            "## Fallback Path", 1
        )[0]
        self.assertIn("selects exactly one normal review skill", review_section)
        self.assertIn("alternatives, not additive defaults", review_section)
        self.assertNotIn("also invoke `$code-review`", review_section)
        self.assertEqual(
            review_section.count("Run exactly one normal closeout review"), 1
        )
        self.assertEqual(
            review_section.count(
                "Run exactly one implementation-boundary adversarial review"
            ),
            1,
        )

        ordered_phrases = (
            "Run exactly one normal closeout review",
            "Fix or disposition its verified findings and rerun relevant validation",
            "Run exactly one implementation-boundary adversarial review",
            "Fix or disposition its verified findings, rerun relevant validation, "
            "and finalize",
        )
        offsets = [review_section.index(phrase) for phrase in ordered_phrases]
        self.assertEqual(offsets, sorted(offsets))

    def test_adversarial_reviews_are_fixed_completed_boundary_events(self):
        self.assertIn(
            "Run exactly one planning-boundary adversarial review after planning "
            "is complete",
            self.skill,
        )
        self.assertIn(
            "Run exactly one implementation-boundary adversarial review after "
            "implementation and normal closeout are complete",
            self.skill,
        )
        planning_section = self.skill.split("### Step 2: Planning Agent", 1)[1].split(
            "### Step 3: Goal And Implementation Agent", 1
        )[0]
        implementation_section = self.skill.split("### Step 4: Review Agents", 1)[
            1
        ].split("## Fallback Path", 1)[0]
        for section in (planning_section, implementation_section):
            self.assertIn(
                "Never re-invoke the adversarial reviewer for that boundary",
                section,
            )
            self.assertIn("including after critical or high findings", section)
        for obsolete in (
            "no independent review",
            "selected independent-review checkpoint",
            "independent_review_checkpoint",
            "independent_review_rationale",
            "Allow at most one re-review",
            "risk-selected planning",
            "risk-selected implementation",
            "risk budget selects",
            "risk budget selected",
            "optional independent reviewer",
        ):
            with self.subTest(obsolete=obsolete):
                self.assertNotIn(obsolete, self.skill)

    def test_completed_adversarial_boundary_is_not_replaceable(self):
        normalized = " ".join(self.skill.split())
        self.assertIn(
            "The continuation-or-replacement rule ends when an adversarial "
            "reviewer returns its completed boundary status.",
            normalized,
        )
        self.assertIn(
            "Context loss, reviewer unavailability, later fixes, changed "
            "artifacts, and failed validation do not reopen that boundary",
            normalized,
        )

    def test_main_agent_owns_workspace_and_goal(self):
        self.assertIn(
            "Worktree selection and Goal activation stay with the main agent",
            self.skill,
        )

    def test_fresh_agents_use_minimal_isolated_context(self):
        self.assertIn("Fresh-Context Launch Contract", self.skill)
        self.assertIn('fork_turns: "none"', self.skill)
        self.assertRegex(self.skill, r"minimal task-local launch\s+packet")
        self.assertRegex(
            self.skill,
            r"applicable lenses and\s+raw artifacts or evidence",
        )
        self.assertRegex(self.skill, r"not the orchestrator's\s+conclusions")
        self.assertIn("persistent grill and implementation agents", self.skill)

    def test_reasoning_profiles_allow_model_selected_maximum(self):
        self.assertIn(
            "Let the phase-launching model select any supported reasoning effort",
            self.skill,
        )
        self.assertIn("`xhigh` and `max` are valid choices", self.skill)
        self.assertIn("they do not require a prior failed attempt", self.skill)
        self.assertIn("Do not impose a global ceiling", self.skill)

    def test_phase_launches_set_and_record_actual_profiles(self):
        self.assertIn(
            "For every supported phase launch, explicitly set both `model` and",
            self.skill,
        )
        self.assertIn("`reasoning_effort`", self.skill)
        for field in (
            "`actual_model`",
            "`actual_reasoning_effort`",
            "`unavailable_capability_fallback`",
        ):
            with self.subTest(field=field):
                self.assertIn(field, self.skill)

    def test_independent_review_has_provider_fallbacks(self):
        self.assertRegex(
            self.skill,
            r"Absence from the\s+native subagent picker is not evidence",
        )
        self.assertIn(
            "Do not launch a same-provider reviewer before completing",
            self.skill,
        )
        self.assertIn(
            "fresh isolated session from the same provider",
            self.skill,
        )
        self.assertIn("record the reduced independence", self.skill)
        self.assertIn(
            "For critical work, the main agent decides whether",
            self.skill,
        )
        self.assertIn(
            "requires explicit user waiver",
            self.skill,
        )

    def test_generated_directory_is_not_a_public_skill_name(self):
        self.assertNotIn("$gen/", self.skill)

    def test_metadata_advertises_fixed_review_lifecycle(self):
        metadata = read_repo_file("grill-plan-build/agents/openai.yaml")
        self.assertIn(
            "fixed review lifecycle",
            metadata,
        )
        self.assertNotIn("risk-based review budget", metadata)

    def test_human_briefing_runs_after_final_review(self):
        briefing_section = self.skill.split(
            "### Step 5: Human Implementation Briefing", 1
        )[1].split("## Fallback Path", 1)[0]
        self.assertIn("`$explain-implementation`", briefing_section)
        self.assertIn('fork_turns: "none"', briefing_section)
        self.assertIn(
            ".agent/work/<slug>/implementation-briefing.html",
            briefing_section,
        )
        self.assertIn("Never stage or commit it", briefing_section)
        self.assertIn("`git status --short` is unchanged", briefing_section)
        self.assertIn("not another review event", briefing_section)
        self.assertIn("disappears with its worktree", briefing_section)

        review_offset = self.skill.index(
            "Run exactly one implementation-boundary adversarial review"
        )
        briefing_offset = self.skill.index("### Step 5: Human Implementation Briefing")
        self.assertLess(review_offset, briefing_offset)

    def test_human_plan_review_precedes_planning_adversarial_review(self):
        planning_section = self.skill.split("### Step 2: Planning Agent", 1)[1].split(
            "### Step 3: Goal And Implementation Agent", 1
        )[0]
        for phrase in (
            ".agent/work/<slug>/plan-review.html",
            "`decision.md` and `execplan.md` remain authoritative",
            "explicit plan approval",
            "`plan-approval`",
            "current plan revision",
            "foreground poll",
            "same planning agent",
            "Never stage or commit the plan-review page",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, planning_section)

        review_offset = planning_section.index("explicit plan approval")
        adversarial_offset = planning_section.index(
            "Run exactly one planning-boundary adversarial review"
        )
        self.assertLess(review_offset, adversarial_offset)

    def test_lavish_contract_is_required_and_tracks_upstream_cli(self):
        normalized_skill = " ".join(self.skill.split())
        for phrase in (
            LAVISH_CLI,
            "loopback",
            "Never invoke `lavish-axi share`",
            "no remote CDN or sidecar assets",
            "Lavish is required",
            "stop the current phase as blocked",
            "main agent owns every Lavish foreground poll",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, normalized_skill)

        self.assertNotIn("lavish-axi@", self.skill)
        self.assertNotIn("direct-browser fallback", self.skill)
        self.assertNotIn("or from the user in chat", self.skill)
        self.assertNotIn("collect the same explicit approval in chat", self.skill)
        self.assertNotIn("npx -y lavish-axi share", self.skill)


class ExplainImplementationPolicyTest(unittest.TestCase):
    def setUp(self):
        self.skill = read_repo_file("explain-implementation/SKILL.md")
        self.metadata = read_repo_file("explain-implementation/agents/openai.yaml")

    def test_is_local_single_file_human_artifact(self):
        self.assertIn(
            ".agent/work/<slug>/implementation-briefing.html",
            self.skill,
        )
        self.assertIn("never stage or commit the page", self.skill)
        self.assertIn("create no sidecar files", self.skill)
        self.assertIn("keep Git status unchanged", self.skill)
        self.assertIn("removing the worktree removes the page", self.skill)

    def test_reconstructs_from_final_source_in_fresh_context(self):
        self.assertIn("one fresh context", self.skill)
        self.assertIn("final diff or commit", self.skill)
        self.assertIn("Prefer final code over planned code", self.skill)
        self.assertIn("repository-relative source reference", self.skill)
        self.assertIn("real repository file", self.skill)
        self.assertIn("It is not another review event", self.skill)

    def test_requires_interactive_and_browser_validation(self):
        for phrase in (
            "component map",
            "happy-path trace",
            "retrieval questions",
            "selecting a component",
            "stepping forward and backward",
            "keyboard focus",
            "390px and 1280px",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, self.skill)

    def test_bundles_renderer_schema_template_and_metadata(self):
        for path in (
            "explain-implementation/scripts/render_briefing.py",
            "explain-implementation/references/briefing-schema.md",
            "explain-implementation/assets/briefing-template.html",
        ):
            with self.subTest(path=path):
                self.assertTrue((ROOT / path).is_file())
        self.assertIn("$explain-implementation", self.metadata)

    def test_renderer_produces_the_required_lavish_artifact(self):
        normalized_skill = " ".join(self.skill.split())
        for phrase in (
            LAVISH_CLI,
            "renderer produces the source HTML",
            "Lavish is the required review and ownership interface",
            "single-file artifact boundary",
            "foreground poll",
            "stop with the ownership phase blocked",
            "Never invoke `lavish-axi share`",
            "user ends the session",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, normalized_skill)

        self.assertNotIn("lavish-axi@", self.skill)
        self.assertNotIn("direct-browser fallback", self.skill)
        self.assertLess(
            self.skill.index("python3 scripts/render_briefing.py"),
            self.skill.index(f"{LAVISH_CLI} <briefing-path>"),
        )


class AdversarialReviewPolicyTest(unittest.TestCase):
    def setUp(self):
        self.skill = read_repo_file("adversarial-review/SKILL.md")
        self.metadata = read_repo_file("adversarial-review/agents/openai.yaml")

    def test_prefers_cross_provider_then_fresh_same_provider(self):
        self.assertIn("different provider", self.skill)
        self.assertIn("fresh isolated session from the same provider", self.skill)
        self.assertIn("reduced independence", self.skill)
        self.assertIn("Cross-provider review is the default requirement", self.skill)
        self.assertRegex(
            self.skill,
            r"Absence from the native subagent\s+picker does not establish",
        )
        self.assertIn(
            "Do not launch a same-provider reviewer before completing",
            self.skill,
        )

    def test_runs_one_reviewer_without_nested_competition(self):
        self.assertIn("Run one fresh reviewer", self.skill)
        self.assertIn("Do not spawn nested reviewers by default", self.skill)
        self.assertNotIn("ask two independent subagents", self.skill)
        self.assertNotIn("whoever finds the largest number", self.skill)

    def test_one_invocation_ends_with_validation_not_re_review(self):
        self.assertIn(
            "One invocation evaluates one completed planning or implementation "
            "boundary.",
            self.skill,
        )
        self.assertIn(
            "rerun relevant validation, and return control without re-invoking "
            "adversarial review for that boundary",
            self.skill,
        )
        self.assertNotIn("Re-review at most once", self.skill)
        self.assertNotIn("risk-gated by its caller", self.skill)

    def test_status_records_provider_and_independence(self):
        for field in (
            "AUTHOR_PROVIDER:",
            "REVIEWER_PROVIDER:",
            "REVIEWER_MODEL:",
            "INDEPENDENCE:",
        ):
            with self.subTest(field=field):
                self.assertIn(field, self.skill)

    def test_core_skill_and_metadata_are_provider_neutral(self):
        combined = self.skill + self.metadata
        self.assertNotIn("Use Claude as", combined)
        self.assertNotIn("artifacts Codex produced", combined)
        self.assertNotIn("with Claude", self.metadata)

    def test_provider_recipes_are_bundled(self):
        path = ROOT / "adversarial-review/references/provider-reviewers.md"
        self.assertTrue(path.is_file())

    def test_external_adapters_are_discovered_before_fallback(self):
        adapters = read_repo_file(
            "adversarial-review/references/provider-reviewers.md"
        )
        self.assertIn("Read this reference during reviewer discovery", adapters)
        self.assertIn("`command -v claude`", adapters)
        self.assertIn("minimal read-only smoke", adapters)
        self.assertIn("Native-picker absence alone never permits fallback", adapters)

    def test_anthropic_adapter_preserves_access_and_profile(self):
        adapter = read_repo_file("adversarial-review/references/provider-reviewers.md")
        self.assertIn(
            "inherit the user's configured provider access by default", adapter
        )
        for credential in (
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
        ):
            with self.subTest(credential=credential):
                self.assertNotIn(f"-u {credential}", adapter)
        self.assertIn('"${REVIEWER_MODEL:-}"', adapter)
        self.assertIn('"${REVIEWER_EFFORT:-}"', adapter)
        self.assertIn('--model "$REVIEWER_MODEL"', adapter)
        self.assertIn('--effort "$REVIEWER_EFFORT"', adapter)
        self.assertNotIn("--model claude-", adapter)
        self.assertNotIn("--effort max", adapter)


class FrontendDesignPolicyTest(unittest.TestCase):
    def setUp(self):
        self.skill = read_repo_file("frontend-design/SKILL.md")

    def test_current_agent_owns_implementation(self):
        self.assertIn("current implementation agent owns", self.skill)
        self.assertNotIn("Use Codex as", self.skill)
        self.assertNotIn("implementer: codex", self.skill)

    def test_frontend_review_consumes_each_fixed_boundary_event(self):
        planning_section = self.skill.split("## Planning Pass", 1)[1].split(
            "## Implementation Pass", 1
        )[0]
        review_section = self.skill.split("## Independent Frontend Review", 1)[1]
        self.assertIn(
            "consumes the parent workflow's one planning-boundary adversarial event",
            planning_section,
        )
        self.assertIn(
            "consumes the parent workflow's one implementation-boundary "
            "adversarial event",
            review_section,
        )
        for section in (planning_section, review_section):
            self.assertIn(
                "does not add a second generic adversarial review",
                section,
            )
            self.assertIn(
                "Never re-invoke the adversarial reviewer for that boundary",
                section,
            )
        for obsolete in (
            "optional and follows the parent risk budget",
            "selected independent-review checkpoint",
            "risk budget selected",
            "risk budget selects",
        ):
            with self.subTest(obsolete=obsolete):
                self.assertNotIn(obsolete, self.skill)

    def test_reviewer_selection_is_provider_neutral(self):
        self.assertIn("Invoke `$adversarial-review`", self.skill)
        self.assertNotIn("Invoke Claude", self.skill)
        self.assertNotIn("ask Claude", self.skill)


if __name__ == "__main__":
    unittest.main()
