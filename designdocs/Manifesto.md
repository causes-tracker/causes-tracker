# Causes

A bug tracker built to free developers from the tedium of manual assessment and 'bug gardening'.
Uses automation and statistics to present information about the project and the experiences users are having.
Bridges the gap between users and developers by explicitly gathering symptoms and signs from users, while letting developers track proposed fixes separately.
Inspired by many conversations around what bug trackers need to deliver for projects.

Causes models bug reports as the interaction between four separate things:

- A project timeline which records things like releases, commits and the like.
  This takes the place of the 'project' or 'component' in other bug tracking tools.
- Signs: observed issues with the project.
  For instance, a segfault on the 3/4/2013 by version 0.2 of the project is a sign of a problem.
  Signs often live in different systems (e.g. Sentry, Datadog, Honeybadger) and Causes merely stores summary or reference information to them.
  Signs are generally worked with in aggregate.
- Symptoms: human reported and discussed issues with the project.
  These take the place of regular bug reports in tools like Bugzilla and Launchpad.
  'I would like to be able to do X' or 'When I try Y this weird thing happens' are typical symptoms.
  Workarounds and ways to mitigate typically get written about in the symptom.
- Plans which document intended changes to the project.
  Sometimes called specifications or story cards.
  Plans may stand alone or be linked to Symptoms or Signs that they are intended to address.
  Plans can range from very small — indeed trivial — to large complex documents with numerous dependencies.
  Developer discussion about diagnosis and investigation would typically occur on a plan.

## Why?

Good bug trackers make a big difference to projects.
They help users communicate with developers and find solutions/workarounds to issues they have.
They help developers find out about issues with their products and converge on good solutions.
They help project managers assess the health of the project and plan/manage releases; they help multiple service providers coordinate work on a project.

At the time of writing there are many bug trackers that do a comprehensive job, but most either are too complex or still not-quite-right.

We are arrogant enough to think we can do better.

Should we contribute our ideas to an existing tracker like Launchpad, Trac or Bugzilla?
Perhaps.
On the plus side existing implementations have existing communities, code, infrastructure and user bases.
On the down side they have existing communities, code, infrastructure and users that provide a brake on radical do-overs — and what we are proposing is in some ways very different.
If we provide a minimal implementation that gathers sufficient interest existing tools may gather the social buy-in necessary to change — and we could stop our new tool.
Win.
Or we might gather a community to maintain it and just be another (better) tool.
Either way, doing a new tool seems like less overall effort, and since we have a significant number of 'what if' questions to answer, the easier we can answer them the better.

## Minimum viable product

**Option A** — the smallest implementation we can use ourselves:

1. Persistent storage (so data entered is kept)
2. Plans, Project and Symptom objects (so that the basic workflow is encodable).
   Signs — later, perhaps a specialised Symptom?
3. An interface of some sort.
   Text files for editing and a CLI query interface may be enough to see if there is value.

> **Discussion:** Martin Pool notes a primitive UI may not be enough to validate the structural hypotheses.
> The key question is at what point users will adopt it — likely when a specific need is served, e.g. deduplication or offline replication.

**Option B** — import-first:

1. Persistent storage
2. Ability to import an existing bug database and run automated analysis over it, split bugs into plans etc.
3. An interface of some sort — CLI may be enough for initial validation.

---

## Features

### Dedicated user, developer and project manager interfaces

Users are usually not really interested in bug reports.
Users are interested in:

- Solving/working around the problem they have encountered
- Knowing if it is fixed in a later release
- Assessing if/when they will be able to get a fix (that hasn't been written yet)
- Obtaining developer time to fix their problem

Tracking symptoms and signs separately from proposed changes provides dedicated interfaces that meet users' needs, developers' needs, and the needs of folk that do planning.
The interfaces should be fluid, not rigidly partitioned — people wear multiple hats, especially on small projects.
\[not validated — experience with Bazaar, Ubuntu etc was a strong indicator\]

### Sensible assumptions

In the absence of explicit decisions, Causes makes sensible assumptions — and updates those as the facts change.
For instance, a symptom many users are experiencing is assumed to be more important to the project than one that only a single user has reported.
If a developer overrides that, the override sticks until removed.
\[Importance: not validated\]

### Does one thing well

Causes is a new take on bug trackers.
This is a rich subject that is worth doing well.
Causes is not a project management tool, nor a kanban tool, nor a VCS history viewer.
Causes offers an API permitting it to trigger events in other systems, and likewise be updated in response to events in other systems.
We consider it a bug if slick automation cannot be easily created to connect Causes to services such as VCS hosting, CI systems, crash reporting systems.

> **Discussion:** Causes does need to consume timelines to infer well, but it doesn't need to own releases the way Launchpad does.
> It needs to know which plans and symptoms are relevant to which versions — that's not strictly tied to releases.

### Work with your bugs on the road

We've all been in the situation of needing to work with bugs at a conference with poor connectivity, or copy a bunch of symptoms for reproduction of a problem while travelling.
Causes allows you to run a local Causes repository that integrates with your project's main online repository.

> **Discussion:** Martin Pool noted offline was a lower priority in 2013 as connectivity improved.
> The counter-argument: offline support is a natural consequence of the distributed/replication architecture needed for other goals — private deployments, external data sources, graceful degradation.
> If we need most of the pieces anyway, we should do it well.

### Work privately with a public project's bugs

Many organisations work on public projects but cannot share all their discussion about particular issues with the public project.
Wouldn't it be great to use the same bug tracking software upstream and locally, with a full copy of the public data set mirrored into your local repository?
Causes supports this as just another case of distributed data.

### Integrates with your existing data

There are many sources of data that help manage a bug report.
Commits to VCS can be excluded as causes if they happened after the report was filed.
Crash database reports (from systems like Sentry or Datadog) can provide backtraces and variable values.
Bug reports in distribution trackers can provide more context or reproduction instructions.
Commits can indicate that a bug is probably fixed.
The same core logic that drives offline access and behind-the-firewall replication is used to work with other data sources.

> **Discussion:** Jelmer Vernooij raised the question of data ownership — does Causes pull all external data into its own store, or just reference it?
> The working answer: take a strong SOA approach; assume nothing is copied in except when selected for replication/federation.

### Manage work in progress

While Causes is not a Kanban system, it does know about work in progress: plans that have been made but not actioned, plans that are pending review, symptoms where reproduction discussion has stalled, and so on.
Causes shows you a clear list of things you have started but not finished, including things where other people are waiting on you, or you are waiting on other people.
Similarly it can show a project-wide view of in-progress work.
\[unvalidated, but a direct application of the automate-tedious-stuff principle\]

### Clearly shows what you and your collaborators have done

The flip side of knowing what you are doing is knowing what you have done.
Causes can summarise this for you, making it easy to talk about what you have done for a project.
Similarly, see what has happened in the whole project recently, over the last month or year.
\[not validated; keep-aware-of-things is fairly obvious; the advertise-myself aspect less so\]

---

## Implementation principles

1. **Open source.**
   We all have day jobs and are not interested in doing a startup in competition with GitHub Issues, Linear, Jira, Plane etc.
   Apache V2 or MIT licence (see [ADR-005](Decisions.md#adr-005-open-source-licence)).
2. **Clean and lean.**
   We don't want a bag-of-everything to maintain.
   We may end up creating several narrow but cooperating sub-projects (core, notification/subscriptions, web UI).
   Validated by past experience.
3. **Easily deployed.**
   Minimally for local testing via Docker Compose; also for self-hosted production use.
   We don't anticipate running a central service, so if folk cannot deploy it, they cannot use it.
   Validated by past experience.
4. **Easily operated.**
   If it cannot be run by non-developers, we will paint ourselves into a run-a-central-service or only-we-use-it corner.

---

## Project governance

1. Code on GitHub.
2. CI from the get-go (GitHub Actions: lint, test, build, container image).
3. Weak consensus decision making.

---

## Values

1. Only encourage effort that is valuable to projects.
   Wasted effort is something we don't like.

   > **Discussion:** Martin Pool notes effort on bugs that never get done is prima facie wasted; the tool should help avoid it.
   > There is still value in communicating to users about bugs unlikely to be fixed — workarounds, low priority, out of scope.
   > Both positions should be explicitly supported.

2. Respect time and effort involved in contributions from all parties: users, intermediaries, developers, project leaders.
   Encourage people to be good to each other, even though there will be conflicting interests and opinions.
3. Work well for projects with mildly different needs.
   Not validated; can be validated as soon as we encounter a 'can't use this without X' situation.
4. Learn and infer as much as possible.
   Not validated; is perhaps the key hypothesis about the design.
   LLMs and modern ML make this significantly more feasible than in 2013.
5. Keep human/intentional things and machine-set/inferred/gathered things separate.
   Applies to storage, UI, and processing — a pervasive separation up and down the stack.
   Not validated; perhaps a derivative of 'learn and infer'.

---

## Archetypes

### Projects

1. **Freegen** — a small personal project, 5–10 open bugs, activity happens rarely.
   Does lightweight releases, does not track multiple stable versions.
   Users are highly technical.
   Bugs are usually shallow and hard to confuse with other bugs.
   No related projects.
2. **Market** — a small commercially funded project, 2000 open bugs, low but regular activity from users.
   Developer activity a few times a week from 3–4 developers.
   Does releases, tracks multiple stable versions.
   Users are often non-technical but sometimes very technical.
   Actively tries to bring users across the fold to become developers.
   Bugs can be very subtle and hard to differentiate from other bugs.
   Has related projects for plugins, and multiple organisations doing commercial support.
   Bugs often require private data of users to reproduce.
3. **BigVector** — a big commercially funded project with 5000 open bugs, activity from hundreds of users a week, and likewise from hundreds of developers.
   Does releases, tracks multiple stable versions.
   Users are usually technical but not aware of project internals.
   Bugs are generally easy to differentiate.
   Has related projects for different facets of the project, multiple organisations doing commercial development and support.
   Bugs rarely require private data to reproduce, but the project has regular security vulnerabilities that have to be disclosed with care.

### Users

1. **Fred** — a user of Market.
   Not very technical, but enthusiastic and happy to try new code branches.
   Fred has a couple of bugs open with Market that are driving him up the wall.
2. **Gary** — a user of BigVector.
   Has a commercial support contract with BigVendor, 4 open bugs that affect him, and is a personal contributor to Market.
3. **Jo** — the developer of Freegen.
   Works on it a few hours a month, as time permits.
4. **Mary** — a developer on BigVector, working for BigVendor.
   Spends most of her time writing code with some planning and user support time thrown in.
   Deals with private data from BigVendor clients when needed, and is part of the release team for BigVector.

### Organisations

1. **Speculative** — a tech company that funds Market.
   Drives Market as a project and collaborates with ServiceCo supporting users of Market.
2. **ServiceCo** — a systems integrator with users using Market.
   Manages the bugs filed by their users and takes care of analysing and fixing them.
3. **SwitchCo** — a network switch vendor that contributes to the development of BigVector, has users using it and provides support to them.
   Manages tickets from their users in a private system.
4. **MachineCo** — a PC server vendor that contributes to the development of BigVector, has users using it and supports them.
   Also manages tickets in private.

---

## Prior art / inspiration

- **Launchpad** has a very good bug tracker.
  Its UI is very complex, it is very hard to run private instances, has a very limited notification/event raising mechanism, and no machine learning embedded in it.
  It conflates work tracking and planning.
  LP is very straightforward to use, with a battle-tested balance between capability and clutter.
  LP also models the needs of Linux distributions well — quite unique in that regard.
- **GitHub Issues / GitLab Issues** — far more capable than in 2013; now a serious comparison point.
- **Linear** — modern, fast, opinionated issue tracker with good developer ergonomics.
- **Plane** — single-vendor AGPL Linear alternative; good reference for self-hostable modern trackers, but the licence is a compliance trap rather than genuine open source.
- **Pivotal Tracker** — an agile story tracker, not a bug tracker (though it can double for small projects).
- **Bugzilla** — well known, but not in any sense simple.
- [AllSoftwareIsBroken](https://web.archive.org/web/20130101000000*/http://pqxx.org/development/libpqxx/wiki/AllSoftwareIsBroken) — an experiment that tries to serve both users and developers in the categorisation of bug reports.
  _(original link dead; use Wayback Machine)_
- [Launchpad bug modelling thread](https://lists.launchpad.net/launchpad-dev/msg07824.html) — the start of a thread about modelling bugs in Launchpad, with some useful principles.
- **git** — a very successful distributed source control system; most interesting as prior art for an effective user model of distributed data.

---

## Axioms

Things we are going to assume but not try to validate:

- We can make something useful in our spare time, despite us all having interesting day jobs.

---

## Non-features

### Cross-project bugs

The cross-project bug feature in Launchpad is arguably a special case of replicated discussion about a distributed bug.
As we don't want to build a single point of service, cross-project bugs as a specific feature are uninteresting.
We may support federation, and if so there is no particular reason to preclude federating with a bug in another project in the same tracker instance.

Cross-project bugs are not a first-class feature.

### Distribution bug tracking

Tracking bugs for distributions is somewhat different to bug tracking for individual projects.
They do interact — many software projects are shipped by distributions, and thus symptoms users experience may be farmed from the shipping distribution's bug trackers.
However, there are at least a thousand times more software projects than Linux distributions, and proportionally fewer chances to be adopted as the bug tracker for a distribution.
It will be nice if what we build is suitable for tracking the bugs of a distribution, and we won't reject volunteer work to make it suitable.

But we won't set out to make it suitable at this stage.

> **Discussion:** Jelmer Vernooij notes packages in a distribution aren't fundamentally different from upstream projects.
> The practical distinction is scale — 20K subprojects in one project — and the intrinsic downstreamness.

### Project timelines, Kanban, VCS integration

While such things are important, we want to keep Causes focused.
We won't add things to Causes itself that are not part of its necessary core features.
We may create a collection of 'you probably want this' API scripts/extensions.
For instance, integrating with GitHub is probably a must-do.

---

## Architecture

Key decisions have been extracted into [Decisions.md](Decisions.md).

### Repository

A repository stores data for a project.
Repositories can be easily and quickly created, and interacted with entirely via a CLI.
CLI access uses local credentials.
Repositories can trigger local hooks in response to events.
They are individually queryable, and have no authentication at the repository layer (authentication is handled by the HTTP daemon).
They are local artefacts but may be configured with network identities (HTTP, SMTP etc) to allow push notifications to reach them for replicated items.
Repositories store only their replication subscription configuration, not the list of subscribers.
HTTP notifications use WebSub content notifications (generated only when the HTTP daemon is running).

### Subscriptions

Subscription information is stored logically separately from the repository, though it may be easiest to manage within the repository store.
Subscriptions map any notifiable object to a notification endpoint (such as an email address), possibly with wildcards.

### Notification

For HTTP notifications, WebSub updates are generated for predictable feeds: one for the object itself, one for each container the object is in, one for any aggregates.
The issue of updates for search results is not addressed.

HTTP webhooks are also supported; they can be recorded in the subscription store and delivered by the repository hook that calls into the notification system.

A dedicated daemon provides websocket streams (or Server-Sent Events) for attached browser clients.
This daemon is stateless and easily scaled — it proxies between browser clients and WebSub, and the client requests appropriate feeds based on what the HTTP UI instructs.

For SMTP, a local repository hook delivers new content notifications to an SMTP handler, which then consults the subscription store to determine where to send the notification.

#### HTTP daemon

A lightweight HTTP daemon provides HTTP API services for multiple repositories and multiple subscription stores.
On startup this daemon establishes WebSub subscriptions to remote repositories for all local subscriptions and checks for missed notifications.
Once started the daemon answers content requests from WebSub hubs as well as serving the browser UI.
User authentication is handled here.
This daemon is stateless — several can be run in parallel for HA or performance.

#### SMTP

Similar to HTTP, but probably no daemon — just a handler for incoming mail.
Routes incoming notifications from subscribed SMTP-only systems (e.g. Bugzilla) into a WebSub handler that will poll and update replicated content appropriately.

### UI

#### Browser UI

RESTful API to the server, JSON representation of data.
Atom feeds for most everything.
Server-side rendering of static views for search engines.
Websockets or SSE to notify clients of interesting events.
New browser sessions establish subscriptions for everything the user is subscribed to, and ask for an initial push of interesting content.

Framework choice is TBD (see [ADR-008](Decisions.md#adr-008-language-stack-open)).
Original design specified Handlebars templates; current alternatives include HTMX, React, Vue, and Svelte.

#### SMTP UI

Probably limited to notifications.
If an email-based workflow is added later, it would be a responsibility of the incoming SMTP handler.

#### CLI

Local client operations form the core of the system — and may be the mechanism the HTTP daemon builds on, keeping things in clean layers.

Operations:

- `init` — initialise a project repository
- `symptom add` — add a symptom
- `sign add` — add a sign
- `plan add` — add a plan
- `timeline add` — add a timeline event: `[subject, verb, date, details, url]`
- `timeline query` — query the timeline
- `edit` — edit a sign/symptom/plan
- `comment` — comment on a sign/symptom/plan/timeline event
- `display` — display an object
- `link` / `unlink` — link a plan to an aggregate symptom/sign
- `aggregate` — aggregate two symptoms or two signs

---

## Missing / open items (2026 update)

The following were not addressed in the original 2013 design and need decisions before implementation:

- **Language and stack** — see [ADR-008](Decisions.md#adr-008-language-stack-open)
- **Security model** (authentication, authorisation, private disclosure workflow) — see [ADR-010](Decisions.md#adr-010-security-model)
- **API specification format** (recommend OpenAPI 3.x) — see [ADR-009](Decisions.md#adr-009-api-specification-format-protobuf)
- **Deployment** (Docker Compose for local dev; OCI containers for production) — see [Design-Choices.md](Design-Choices.md#deployment)
- **CI/CD pipeline** (GitHub Actions: lint, test, build, publish container image)
- **Data schema** — the four entity types are described in prose; a concrete schema definition is needed
- **LLM-assisted triage** — duplicate detection and auto-triage using LLMs is now practical and worth revisiting as part of the "learn and infer" value
