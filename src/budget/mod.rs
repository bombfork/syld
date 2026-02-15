// SPDX-License-Identifier: GPL-3.0-or-later

//! Budget management and donation plan generation.
//!
//! Given a user's monthly/yearly budget and a list of discovered projects,
//! this module generates a donation plan that distributes the budget across
//! projects according to the chosen allocation strategy.

use serde::{Deserialize, Serialize};

use crate::config::{BudgetConfig, Cadence};
use crate::project::{FundingChannel, UpstreamProject};

/// A complete donation plan for a budget period.
#[derive(Debug, Serialize, Deserialize)]
pub struct DonationPlan {
    pub allocations: Vec<Allocation>,
}

/// A single allocation in a donation plan.
#[derive(Debug, Serialize, Deserialize)]
pub struct Allocation {
    /// The project to donate to
    pub project: UpstreamProject,

    /// Amount per donation
    pub amount: f64,

    /// Donate every N months
    pub every_n_months: u32,

    /// Suggested funding channel
    pub via: Option<String>,

    /// Reason for including this project (e.g. "top dependency", "most used")
    pub reason: Option<String>,
}

/// A record of a completed donation.
#[derive(Debug, Serialize, Deserialize)]
pub struct DonationRecord {
    /// Database row ID
    pub id: i64,

    /// URL of the project that received the donation
    pub project_url: String,

    /// Amount donated
    pub amount: f64,

    /// Currency code (e.g. "USD", "EUR")
    pub currency: String,

    /// When the donation was made
    pub donated_at: chrono::DateTime<chrono::Utc>,

    /// Funding channel used
    pub via: Option<String>,

    /// Free-form notes
    pub notes: Option<String>,
}

/// How to distribute the budget across fundable groups.
#[derive(Debug, Clone, Copy)]
pub enum AllocationStrategy {
    /// Divide the budget equally among all groups.
    Equal,
    /// Weight each group by its total star count (minimum weight 1).
    Weighted,
}

/// A group of related projects (same org/ancestor) that can receive funding.
pub struct FundableGroup {
    /// Org/ancestor URL or project name used as the display label.
    pub label: String,
    /// Constituent projects in this group.
    pub projects: Vec<UpstreamProject>,
    /// Deduplicated funding channels across the group.
    pub funding: Vec<FundingChannel>,
    /// Sum of stars across projects in the group.
    pub total_stars: u64,
}

/// Generate a donation plan from a budget, a set of fundable groups, and a strategy.
pub fn generate_plan(
    budget: &BudgetConfig,
    groups: Vec<FundableGroup>,
    strategy: AllocationStrategy,
) -> DonationPlan {
    let amount = budget.amount.unwrap_or(0.0);
    let every_n_months: u32 = match budget.cadence {
        Cadence::Monthly => 1,
        Cadence::Yearly => 12,
    };

    if groups.is_empty() || amount <= 0.0 {
        return DonationPlan {
            allocations: vec![],
        };
    }

    let allocations = match strategy {
        AllocationStrategy::Equal => {
            let per_group = amount / groups.len() as f64;
            groups
                .into_iter()
                .map(|g| make_allocation(g, per_group, every_n_months))
                .collect()
        }
        AllocationStrategy::Weighted => {
            let weights: Vec<u64> = groups.iter().map(|g| g.total_stars.max(1)).collect();
            let total_weight: u64 = weights.iter().sum();
            groups
                .into_iter()
                .zip(weights)
                .map(|(g, w)| {
                    let share = amount * (w as f64 / total_weight as f64);
                    make_allocation(g, share, every_n_months)
                })
                .collect()
        }
    };

    DonationPlan { allocations }
}

fn make_allocation(group: FundableGroup, amount: f64, every_n_months: u32) -> Allocation {
    let via = group.funding.first().map(|f| f.url.clone());
    let reason = if group.projects.len() == 1 {
        Some("1 installed package".to_string())
    } else {
        Some(format!("{} installed packages", group.projects.len()))
    };

    // Use the group label as the project name; pick the first project's repo_url
    let representative = group
        .projects
        .first()
        .cloned()
        .unwrap_or_else(|| UpstreamProject {
            name: group.label.clone(),
            repo_url: None,
            homepage: None,
            licenses: vec![],
            funding: group.funding.clone(),
            bug_tracker: None,
            contributing_url: None,
            is_open_source: None,
            documentation_url: None,
            good_first_issues_url: None,
            stars: None,
        });

    Allocation {
        project: UpstreamProject {
            name: group.label,
            funding: group.funding,
            ..representative
        },
        amount: (amount * 100.0).round() / 100.0,
        every_n_months,
        via,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::FundingChannel;

    fn make_budget(amount: f64, cadence: Cadence) -> BudgetConfig {
        BudgetConfig {
            amount: Some(amount),
            currency: "USD".to_string(),
            cadence,
        }
    }

    fn make_group(label: &str, stars: u64, num_projects: usize) -> FundableGroup {
        let projects: Vec<UpstreamProject> = (0..num_projects)
            .map(|i| UpstreamProject {
                name: format!("{label}-pkg-{i}"),
                repo_url: Some(format!("https://github.com/{label}/repo-{i}")),
                homepage: None,
                licenses: vec![],
                funding: vec![FundingChannel {
                    platform: "GitHub Sponsors".to_string(),
                    url: format!("https://github.com/sponsors/{label}"),
                }],
                bug_tracker: None,
                contributing_url: None,
                is_open_source: None,
                documentation_url: None,
                good_first_issues_url: None,
                stars: Some(stars / num_projects.max(1) as u64),
            })
            .collect();

        FundableGroup {
            label: label.to_string(),
            projects,
            funding: vec![FundingChannel {
                platform: "GitHub Sponsors".to_string(),
                url: format!("https://github.com/sponsors/{label}"),
            }],
            total_stars: stars,
        }
    }

    #[test]
    fn equal_plan_divides_evenly() {
        let budget = make_budget(30.0, Cadence::Monthly);
        let groups = vec![
            make_group("alpha", 100, 2),
            make_group("beta", 200, 1),
            make_group("gamma", 50, 3),
        ];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Equal);
        assert_eq!(plan.allocations.len(), 3);
        for alloc in &plan.allocations {
            assert!((alloc.amount - 10.0).abs() < 0.01);
            assert_eq!(alloc.every_n_months, 1);
        }
    }

    #[test]
    fn weighted_plan_by_stars() {
        let budget = make_budget(100.0, Cadence::Monthly);
        let groups = vec![make_group("small", 10, 1), make_group("big", 90, 1)];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Weighted);
        assert_eq!(plan.allocations.len(), 2);

        let small = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "small")
            .unwrap();
        let big = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "big")
            .unwrap();

        assert!((small.amount - 10.0).abs() < 0.01);
        assert!((big.amount - 90.0).abs() < 0.01);
    }

    #[test]
    fn yearly_cadence_sets_every_12_months() {
        let budget = make_budget(120.0, Cadence::Yearly);
        let groups = vec![make_group("proj", 50, 1)];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Equal);
        assert_eq!(plan.allocations.len(), 1);
        assert_eq!(plan.allocations[0].every_n_months, 12);
    }

    #[test]
    fn group_with_zero_stars_gets_minimum_weight() {
        let budget = make_budget(100.0, Cadence::Monthly);
        let groups = vec![
            make_group("zero-stars", 0, 1),
            make_group("some-stars", 99, 1),
        ];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Weighted);
        assert_eq!(plan.allocations.len(), 2);

        let zero = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "zero-stars")
            .unwrap();
        let some = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "some-stars")
            .unwrap();

        // zero-stars gets weight 1, some-stars gets weight 99, total weight 100
        assert!((zero.amount - 1.0).abs() < 0.01);
        assert!((some.amount - 99.0).abs() < 0.01);
    }

    #[test]
    fn empty_groups_produces_empty_plan() {
        let budget = make_budget(100.0, Cadence::Monthly);
        let plan = generate_plan(&budget, vec![], AllocationStrategy::Equal);
        assert!(plan.allocations.is_empty());
    }

    #[test]
    fn zero_budget_produces_empty_plan() {
        let budget = make_budget(0.0, Cadence::Monthly);
        let groups = vec![make_group("proj", 50, 1)];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Equal);
        assert!(plan.allocations.is_empty());
    }

    #[test]
    fn allocation_has_via_from_funding() {
        let budget = make_budget(10.0, Cadence::Monthly);
        let groups = vec![make_group("proj", 50, 1)];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Equal);
        assert_eq!(
            plan.allocations[0].via.as_deref(),
            Some("https://github.com/sponsors/proj")
        );
    }

    #[test]
    fn reason_reflects_package_count() {
        let budget = make_budget(10.0, Cadence::Monthly);
        let groups = vec![make_group("single", 10, 1), make_group("multi", 10, 3)];
        let plan = generate_plan(&budget, groups, AllocationStrategy::Equal);

        let single = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "single")
            .unwrap();
        let multi = plan
            .allocations
            .iter()
            .find(|a| a.project.name == "multi")
            .unwrap();

        assert_eq!(single.reason.as_deref(), Some("1 installed package"));
        assert_eq!(multi.reason.as_deref(), Some("3 installed packages"));
    }
}
