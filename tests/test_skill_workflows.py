import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


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
            "optional independent reviewer",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, self.skill)

    def test_standard_path_has_one_improvement_and_one_review(self):
        self.assertIn(
            "exactly one `$execplan-improve` attempt",
            self.skill,
        )
        self.assertIn("| Standard |", self.skill)
        self.assertIn("no independent review", self.skill)
        self.assertNotIn("Default to 3", self.skill)
        self.assertNotIn("up to 3 times", self.skill)

    def test_normal_review_skills_are_alternatives(self):
        self.assertIn("selects exactly one normal review skill", self.skill)
        self.assertIn("alternatives, not additive defaults", self.skill)
        self.assertNotIn("also invoke `$code-review`", self.skill)
        self.assertNotIn("Then invoke `$adversarial-review`", self.skill)

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

    def test_elevated_review_checkpoint_and_rationale_persist(self):
        self.assertIn(
            "persist both the selected checkpoint and its rationale", self.skill
        )
        self.assertIn('"independent_review_checkpoint"', self.skill)
        self.assertIn('"independent_review_rationale"', self.skill)
        self.assertRegex(self.skill, r"before\s+the review begins")

    def test_reasoning_profiles_do_not_default_to_maximum(self):
        self.assertIn("Do not prescribe `xhigh` or `max`", self.skill)

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


class AdversarialReviewPolicyTest(unittest.TestCase):
    def setUp(self):
        self.skill = read_repo_file("adversarial-review/SKILL.md")
        self.metadata = read_repo_file("adversarial-review/agents/openai.yaml")

    def test_prefers_cross_provider_then_fresh_same_provider(self):
        self.assertIn("different provider", self.skill)
        self.assertIn("fresh isolated session from the same provider", self.skill)
        self.assertIn("reduced independence", self.skill)

    def test_runs_one_reviewer_without_nested_competition(self):
        self.assertIn("Run one fresh reviewer", self.skill)
        self.assertIn("Do not spawn nested reviewers by default", self.skill)
        self.assertNotIn("ask two independent subagents", self.skill)
        self.assertNotIn("whoever finds the largest number", self.skill)

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

    def test_frontend_review_consumes_parent_checkpoint(self):
        self.assertIn(
            "satisfies the parent workflow's selected independent-review checkpoint",
            self.skill,
        )
        self.assertIn("Do not run a second independent review", self.skill)

    def test_reviewer_selection_is_provider_neutral(self):
        self.assertIn("Invoke `$adversarial-review`", self.skill)
        self.assertNotIn("Invoke Claude", self.skill)
        self.assertNotIn("ask Claude", self.skill)


if __name__ == "__main__":
    unittest.main()
