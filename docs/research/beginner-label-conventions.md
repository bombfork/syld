# Beginner-Friendly Label Conventions Across Git Forges

A comprehensive research document comparing label conventions for beginner-friendly issues across major git forges and platforms.

## Executive Summary

Different git forges have established label conventions to help newcomers find suitable issues to contribute to. While there are some common patterns, each platform has its own approach and terminology. This document serves as a reference for understanding and implementing beginner-friendly label systems.

---

## 1. GitHub

### Standard Label Names

GitHub maintains a set of default labels that are automatically available in new repositories:

- **`good first issue`** - The primary label for beginner-friendly issues
- **`help wanted`** - Indicates that a maintainer wants help on an issue

### Special Features & Handling

- **Contribute Page Population**: Issues labeled with `good first issue` are used to populate the repository's "Contribute" page
- **Algorithm-Driven Discovery**: GitHub uses an algorithm to determine the most approachable issues in each repository and surfaces them across the platform
- **Label Recognition**: GitHub specifically recognizes and promotes the `good first issue` label, increasing the likelihood that issues are surfaced to potential contributors
- **Nested Labels**: `good first issue` is typically a subset of `help wanted` - issues can have both labels, with `good first issue` being more specific to newcomers

### Official Documentation

- [Managing labels - GitHub Docs](https://docs.github.com/en/issues/using-labels-and-milestones-to-track-work/managing-labels)
- [Encouraging helpful contributions to your project with labels - GitHub Docs](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/encouraging-helpful-contributions-to-your-project-with-labels)

### Best Practices

- Use `good first issue` for self-contained work that would make a good introduction to project development
- Combine with `help wanted` to indicate maintainer commitment to assisting contributors
- Refer to the Kubernetes contributor guide for comprehensive community standards: [Help Wanted and Good First Issue Labels](https://www.kubernetes.dev/docs/guide/help-wanted/)

---

## 2. GitLab

### Standard Label Names

GitLab does not prescribe specific beginner-friendly label names but provides infrastructure for creating custom labels. When using default generated label sets:

- No explicit "good first issue" equivalent is built-in by default
- Custom labels must be created per project or per group

### Default Label Generation

- GitLab provides a "Generate a default set of labels" feature
- This creates 8 basic labels for initial project categorization
- Default labels include categories like priority levels, but not specifically beginner-focused labels

### Label Organization Features

**Scoped Labels**: GitLab supports scoped labels using `::` syntax:
- Format: `scope::label` (e.g., `workflow::in-review`, `priority::high`)
- Enables mutually exclusive label sets
- Useful for status tracking and team assignment

**Effort Labels**: Some projects use effort-based labeling:
- `Effort: Low`
- `Effort: Medium`
- `Effort: High`

**Priority Labels**:
- `P1` (top priority)
- `P2` (high priority)
- `P3` (medium priority)
- `P4` (low priority)

### Label Naming Conventions

- Keep labels to 25 characters or fewer to avoid UI truncation
- Use scoped labels (with `::`) for related label groups
- Use lowercase with hyphens or spaces

### Official Documentation

- [Labels - GitLab Docs](https://docs.gitlab.com/user/project/labels/)
- [Labels project management guidelines - The GitLab Handbook](https://handbook.gitlab.com/handbook/marketing/project-management-guidelines/labels/)

### Best Practices for Beginner Labels

While GitLab doesn't define a standard, many projects create custom labels such as:
- `help wanted` (using GitHub naming convention)
- `good first issue` (using GitHub naming convention)
- `beginner friendly` or `beginner-friendly`
- `difficulty::low` (using scoped label syntax)

---

## 3. Codeberg / Gitea

### Standard Label Names

**Codeberg's Recommendation**:
- **`help wanted`** - The primary label for beginner-friendly contributions

**Gitea's Approach**:
- Gitea (the underlying platform) provides preset label sets
- Default presets include: "Advanced" (Kind/Bug, Kind/Feature, etc.) and "Default" (bug, duplicate, etc.)
- No built-in beginner-specific labels by default

### Label Templates & Presets

Gitea supports **Advanced Label Templates** via YAML configuration:
- Enables custom label sets at the instance level
- Can be configured globally and applied to all repositories
- Supports scoped labels with `/` syntax (e.g., `priority/high`, `priority/low`)

### Scoped Label Support

Like GitLab, both Codeberg and Gitea support scoped labels:
- Format: `scope/label` (e.g., `priority/high`, `team/front-end`)
- Useful for creating mutually exclusive label sets
- Can represent priority levels, team assignments, or status

### Repository Label Selection

When creating a repository in Gitea/Codeberg:
- Available label sets are shown in the "Issue Labels" option
- Selected label sets are created automatically with the repository
- Can be customized after repository creation

### Official Documentation

- [Labels - Gitea Documentation](https://docs.gitea.com/usage/labels)
- [The Basics of Issue Tracking - Codeberg Documentation](https://docs.codeberg.org/getting-started/issue-tracking-basics/)
- [Introducing new features of labels and projects - Gitea Blog](https://blog.gitea.io/introducing-new-features-of-labels-and-projects/)

### Best Practices

- Start with Codeberg's basic label set, then add custom feature labels as needed
- Use scoped labels (`scope/label`) for better organization
- Create custom YAML label templates for instance-wide consistency

---

## 4. SourceHut

### Label System Overview

SourceHut provides a labels system with the following characteristics:

- **Flexible Color Customization**: Labels can have custom colors in CSS hexadecimal RGB format (`#RRGGBB`)
- **Email-Based Management**: Labels can be added and removed via email by replying with `!label example` on the last line
- **GraphQL API Support**: Full programmatic access to label management through GraphQL API

### Beginner-Friendly Label Conventions

SourceHut does not prescribe a specific default beginner label like GitHub. However, it supports common conventions used across open-source:

- `help wanted` (common pattern)
- `good first bug` (variant of GitHub's `good first issue`)
- `beginner friendly`
- `good for beginners`
- `first time contributor`

### Label Features

- **Label Sets**: Users can create custom sets of labels and view associated tickets directly in the creation UI
- **Color Coding**: Full customization of label appearance with hex color codes
- **API Integration**: Labels are fully accessible through SourceHut's GraphQL API for automation

### Official Documentation

- [SourceHut Documentation - man.sr.ht](https://man.sr.ht/)
- [SourceHut Operational Manual](https://man.sr.ht/ops/)
- [SourceHut API Documentation](https://docs.sourcehut.org/)

### Best Practices

- Establish consistent labeling conventions across your SourceHut projects
- Use email-based label management for lightweight, decentralized workflow
- Consider using `help wanted` to align with broader open-source conventions
- Configure label colors meaningfully to improve visual scanning

---

## Comparative Summary

### Label Naming Conventions

| Platform | Standard Beginner Label | Alternative Forms | Built-in Default |
|----------|------------------------|-------------------|-----------------|
| GitHub | `good first issue` | `help wanted` | Yes (both included in defaults) |
| GitLab | Custom | `help wanted`, `difficulty::low` | No (must create custom) |
| Codeberg/Gitea | `help wanted` | Custom via templates | Gitea: Advanced/Default templates available |
| SourceHut | Custom (conventions vary) | `good first bug`, `beginner friendly` | No (custom only) |

### Key Features Comparison

| Platform | Scoped Labels | Email Management | Algorithm Discovery | Auto-Population | Template System |
|----------|--------------|-----------------|-------------------|-----------------|-----------------|
| GitHub | No | No | Yes | Yes (Contribute page) | Limited |
| GitLab | Yes (`::`) | No | No | No | Moderate |
| Codeberg/Gitea | Yes (`/`) | No | No | No | Yes (YAML templates) |
| SourceHut | No | Yes | No | No | Manual creation |

### Discovery & Promotion

- **GitHub**: Actively promotes `good first issue` across platform UI and discovery features
- **GitLab**: No built-in promotion; relies on project-level label usage
- **Codeberg/Gitea**: No automatic promotion; `help wanted` is recommended by documentation
- **SourceHut**: No automatic promotion; relies on community conventions

---

## Recommendations for Implementation

### For a Multi-Forge Project

1. **Use GitHub conventions as baseline**: `good first issue` and `help wanted` are the most recognized
2. **Adapt for platform constraints**:
   - GitLab: Use `good-first-issue` and `help-wanted` (hyphenated for consistency)
   - Gitea/Codeberg: Use `help wanted` or `good first issue` as configured
   - SourceHut: Use `help wanted` for consistency
3. **Leverage scoped labels where available**: Use `difficulty/low` on GitLab and Gitea for better filtering
4. **Document your conventions**: Make labeling guidelines explicit in CONTRIBUTING.md

### Label Semantics

- **`good first issue`/`good-first-issue`**: For self-contained, well-documented, low-complexity issues suitable for newcomers
- **`help wanted`**: For issues where maintainers are explicitly seeking contributions (can apply to issues of any difficulty)
- **`difficulty/low`** or **`effort: low`**: Supplement beginner labels with difficulty indicators

### Community Alignment

Refer to established community standards:
- Kubernetes: [Help Wanted and Good First Issue Labels](https://www.kubernetes.dev/docs/guide/help-wanted/)
- Common patterns across major open-source projects prioritize `good first issue` recognition

---

## References & Resources

### Official Documentation

- [GitHub: Managing labels](https://docs.github.com/en/issues/using-labels-and-milestones-to-track-work/managing-labels)
- [GitHub: Encouraging helpful contributions](https://docs.github.com/en/communities/setting-up-your-project-for-healthy-contributions/encouraging-helpful-contributions-to-your-project-with-labels)
- [GitLab: Labels](https://docs.gitlab.com/user/project/labels/)
- [GitLab: Label best practices](https://handbook.gitlab.com/handbook/marketing/project-management-guidelines/labels/)
- [Gitea: Labels documentation](https://docs.gitea.com/usage/labels)
- [Codeberg: Issue tracking basics](https://docs.codeberg.org/getting-started/issue-tracking-basics/)
- [SourceHut: man.sr.ht](https://man.sr.ht/)
- [SourceHut: API Documentation](https://docs.sourcehut.org/)

### Community Standards

- [Kubernetes: Help Wanted and Good First Issue Labels](https://www.kubernetes.dev/docs/guide/help-wanted/)

---

**Last Updated**: March 2026
**Scope**: GitHub, GitLab, Gitea, Codeberg, SourceHut
