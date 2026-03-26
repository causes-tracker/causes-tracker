# Raw Discussion (Historical)

> **Note:** This file is historical. It captures raw braindumps and IRC logs from design discussions in 2012–2013. It has not been edited for clarity or correctness.
> Decisions extracted from this material live in [Decisions.md](Decisions.md).
> The canonical design documentation is in [Manifesto.md](Manifesto.md) and [Design-Choices.md](Design-Choices.md).

---

jml dumped the notes from bug tracker yammer on #subunit here.  Since then, it's been added to in lots of ways by lots of people.

## Why?

After using Launchpad's bug tracker, we dislike using other bug trackers. But Launchpad's bug tracker is ugly, mostly unmaintained and tied up with the rest of Launchpad. Maybe it's a good idea to make another bug tracker.

The bug tracker does seem to be one of the things that Launchpad got right.

## Assumptions

* Launchpad is not being usefully maintained
* Launchpad's UI is terrible
* Extracting Launchpad's bug tracker into a separate thing and making it good is more work than making a new good thing
* We can make something useful in our spare time, despite us all having interesting day jobs

## Relevant

mpt's spec for an issue tracker

## Questions

* Are cross-project bugs actually a desirable feature?
* Are distributions too idiosyncratic for a generalized bug tracker? If we were to do this work, would we care about distros?
* What more assumptions?
* What's the minimum viable product?
* What about kanban? Is it even relevant?
* What would a bug tracker designed by David Allen (GTD) look like?

## Braindump

## lifeless

* API, callbacks (whatever sort - pshb, webhooks, don't care)
* evidence-based model
  * Observations are from users
  * Causes are from experts
* Assume sensible behaviour; default open permissions
* Keep out of the way
* Snappy
* Website (but don't care too much about it)
* For the few projects that do become popular, don't assume that 'project priorities' == 'user priorities' == 'contributor priorities'
* 'in trunk' vs 'in a tarball' is still useful for many people
* Don't want
  * a freaking stack freaking exchange ghetto.
  * copy-paste anything ever related to code pushes/releases/
  * bug graffiti (e.g. open/close wars)
* Don't care about
  * release management
  * multiple long lived branches in the bug tracker model
  * subcomponents and other highly refined metadata
    * if it's big enough to need a structured marker, it's big enough to be its own project.

## jelmer

* custom statuses
* distributed (but none of the current ones can do it)
  * don't version all bug data as if it were code
    * i.e. if you pull a bug report from a user you clutter the history of your project with that
    * if you pull a bug report from a user you clutter the history of your project with that
    * if you pull a bug report from a user you clutter the history of your project with that
    * I think it might be more reasonable if you had a separate branch, perhaps in the same git repository for each bug
* the site should be one way of presenting and interacting that data
  * they should not get to own it
  * not be the exclusive way to interact with it like they are with bug data at the moment

## jml

* mass editing
* keyboard bindings
* fewer statuses (as per mpt)
* are cross-project bugs worth it?
* distributed
* offline
* LP is hideous
* Haskell!
* Clojure!
* Keep all the info!
* PyPy!
* Make waste (inventory, hand-offs, waiting) obvious
* Tags are nice, because they establish context. It's often easier after fixing one bug to fix a similarly tagged bug.
* I don't use priorities for my personal TODOs. Why should a bug tracker have them?
  * Now / Soon / Never
* Keyboard bindings
* Mass editing
* API with callbacks

## poolie

* many (probably most) bugs won't get fixed; so avoid requiring or encouraging developers to spend most time on them, or having them get in the way.
* bugs should decay from view if they are not fixed, but still make it possible to dig out old bugs
* good automatic dupe-finding, including retrospectively or interactively hunting for dupes

## ttx

* Looking into a new task tracker to support OpenStack needs (features + bugs)
* Having multiple tasks (project + branch combination) within a single "story" is very useful to us
* Still interested in getting feature tracking in the same framework, but not as a wishlist importance bug. Features can benefit from tasks too (tasks which can affect multiple projects, and supporting multiple items with the same project + branch combination to emulate work items)
* Avoid inconsistencies (bug 'triaged' but without an importance set)
* For objectives management, replacing series nominations (and the confusing series goal vs. target milestone) by a "release radar" flag set by drivers, to highlight things that should be on release radar
* Tasks can share importance
* More at <https://wiki.openstack.org/wiki/Task_Tracker_Requirements>

## Work notes

* Pay someone to do visuals?
* lifeless will do the guts if someone else does the HTTP
* At least want some people in group who care about upper layers / front-end

## Edited conversations

## WONTFIX, priorities, how bugs relate to code, federation

WONTFIX is not a valid bug status for open source projects.  At least, not the way LP defines it.

There is a WONTFIX, in the sense that I can say "I will not merge a change that meets this use case ever."

Federation is one answer to that tension – "oh yeah, well I am publishing a tree that does it" – but it's not a very satisfying answer.

Tie WONTFIX to code?

Perhaps WONTFIX is something that's specific to a particular branch of the code, much like a particular branch can contain a fix for a bug?

Perhaps not. We have code to MITM SSL connections in Squid. If as a project we had said "we consider this immoral and won't support it", then it might be per branch, for-all project official branches, and perhaps even 'for all branches hosted on project servers'. Making WONTFIX specific to code would require speaking about larger entities than just simple branches, which complicates things.

Bug reports might talk about a branch, or a project, or an effort – it's fluffy. Is binding them together necessary to talk about WONTFIX? No.

Track everyone's priorities

Perhaps then we should explicitly track everyone's priorities and present an aggregate?

Interesting actors in a bug report
* a user
* some users
* a volunteer
* some volunteers
* the project

Consider a project member. The project likely has some a priori agenda (whether commercial like LP itself, or opinionated, like testtools)  A project member may well want to track both personal interests and project interests. (NB: I'm not strictly talking priorities here.)

Consider a user.  They probably don't want to track anything, but they want to know if *anyone* has decided they will do this thing so they want [queue depth in days for this bug to be what someone is hacking on].

Consider the project again.  Beyond or complementary to its a priori agenda, it may well want to know what things are most frustrating to its user base.  Which is where tracking incident rates [for crashes] and reporting rates [for frequently requested dups], and 'me wants it too' etc data.

Do I want to track every users priority on every bug in every project? Probably.
Do I want to show it as that? Hell no.

For instance, on openstack, it would be awesome if companies like HP that have their own priorities could just mark up the main with all their desires without conflicting with 'the project' view of the world.

Federation says 'HP should have their own private tracker', but it's unsatisfying because unless you have bi-di sync you lose data.  If you have bi- di sync then you have a base data model to do the sorts of things I express above.

Note that a few nice things fall out of this, if you don't consider showing everyone's values to everyone ever.  In particular, the 'bug graffiti' of someone unprivileged closing a bug becomes more manageable.

A user says 'this is fix released' but you treat that as input to 'what users think' aggregate data rather than a change on the core.

## Raw conversations

```text
[14:41:24] <jelmer> I still really appreciate the Launchpad bug tracker UI; GitHub's is terrible, and so are most others, especially bugzilla
[14:46:05] <jml> yeah
[14:46:10] <jml> it could definitely use some love though
[14:46:20] <jml> mass editing, keyboard bindings, fewer statuses
[14:49:51] <jelmer> custom statuses (-:
[14:52:35] <jml> heh. yes.
[14:53:10] <jml> I'm also not sure "Also affects" is really worth it
[14:53:16] <jml> hmm
[14:53:44] <jml> actually, if I were to make yet another bug tracker, I would absolutely make it distributed & offline
[14:54:17] <jelmer> Oh, definitely
[14:54:26] <jelmer> I would love a distributed bug tracker that works well
[14:54:30] <jelmer> None of the current ones seem to though
[14:54:52] <jelmer> I think Aaron's BugsEverywhere tracker is still alive, but I don't like that it basically versions all bug data
[14:55:47] <jelmer> I would like sites like GitHub (or Launchpad) to just provide a convenient way for presenting and interacting with data
[14:56:53] <jml> why don't you like versioning all bug data?
[14:57:50] <jml> and "just provide a convenient way for presenting and interacting with data" is rather general. it's not a bad statement of the general problem of design in software
[15:11:32] <jelmer> jml: versioning bug data means that if you pull a bug report from a user you clutter the history of your project with that
[15:11:59] <jelmer> and that in order to have a bug be publicly known it has to be pulled by the maintainer of that branch
[15:12:19] <jelmer> to some degree it's the "if all you have is a hammer, everything looks like a nail" kind of solution
[15:12:37] <jml> hmm
[15:12:41] <jelmer> each bug basically has its own history
[15:12:48] <jml> hmm
[15:12:51] <jml> datomic
[15:12:55] <jml> That's what I want to say.
[15:13:05] <jelmer> I think it might be more reasonable if you had a separate branch, perhaps in the same git repository for each bug
[15:13:17] <jelmer> like e.g. gerrit does for its metadata
[15:14:01] * jelmer finds out about datomic
[15:14:04] <jml> The Value of Values
[15:14:13] <jml> watch that
[15:14:17] <jml> http://www.infoq.com/presentations/Value-Values
[15:14:39] <jelmer> jml: "just provide a convenient way for presenting and interacting with data" is rather general indeed, and not very well phrased.
[15:15:17] <jelmer> jml: I guess I mean that these sites should just be *a* way of presenting and interacting that data, they should not get to own it or be the exclusive way to interact with it like they are with bug data at the moment.
[15:15:25] <jml> ah right.
[15:15:28] <jml> primarily an API
[15:15:32] <jml> secondarily a UI
[15:15:53] <jml> jelmer: I'm very interested in this conversation, but I have to go :\
[15:16:16] <jelmer> jml: likewise, we'll pick it up some other time :)
```

```text
[10:11:24] * jml mutters something about launchpad
[10:11:35] <lifeless> I've been using gerrit recently.
[10:11:44] <lifeless> Much less usable in some ways. Better in others.
[10:11:53] <lifeless> But I'm not inclined to rag on Launchpad :)
[10:23:04] <jml> yeah
[10:23:45] <jml> I'm just frustrated that there's all this basic usability stuff that never even got attempted
[10:24:04] <jml> and now it's mega-huge abandoned enterprise software
[20:07:48] <lifeless> is it abandoned ?
[20:43:25] <jelmer> lifeless: it seems largely abandoned; do the minor dents that the remaining maintenance team make count?
[20:44:02] <lifeless> Dunno :)
[20:44:15] <lifeless> There is a long running angst between big teams and small super effective teams.
[20:47:46] <jelmer> fair enough :)
```

```text
[10:31:45] <jml> I might start migrating my interesting +junk to github today
[10:35:42] <jelmer> jml: what are you going to do for bugs?
[10:36:00] <jml> jelmer: what bugs?
[10:36:17] <jml> my software is perfect.
[10:36:21] <jelmer> jml: in projects you're migrating to github?
[10:36:25] <jelmer> heh
[10:36:28] <jml> I have tests to prove it and everything.
[10:36:33] <jelmer> jml: I guess you don't need version control then either ? :P
[10:36:37] <jml> heh.
[10:36:58] <jml> jelmer: I don't know. I think I'll experiment w/ Github issues for one project
[10:37:31] <jml> jelmer: I haven't actually used their bug tracker at all.
[10:37:49] * jelmer found the experience a bit underwhelming
[10:38:17] <jelmer> but let us know how you find it
[10:39:06] <lifeless> jml: its mediocre IME
[10:41:48] <jml> We'll see.
[10:41:53] <jml> Any non-Launchpad suggestions?
[10:41:56] <jelmer> the bug tracker does seem to be one of the things that Launchpad got right
[10:42:17] <jelmer> I feel myself longing back whatever else (bugzilla, google code, trac, github) I use
[10:43:19] <jelmer> jml: not sure; if you're going to use an external thing anyway, why not use Launchpad?
[10:43:41] <jelmer> Some sort of distributed BTS would be nice, but I haven't seen any that I like.
[10:43:42] <jml> jelmer: just curious.
[10:43:58] <jml> jelmer: also (and I know this is very shallow), LP is hideous.
[10:44:39] <jelmer> do you think so?
[10:45:03] <jelmer> maybe I've just been conditioned to consider it okay over the years :)
[10:45:05] <jml> jelmer: "hideous" is over-stating it, but yes, I think it's ugly.
[10:46:01] <jml> I wonder how much work & how much value there'd be in splitting the bug tracker out of Launchpad.
[10:47:01] <jelmer> I'll take Launchpad's layout over that of bugzilla or gitorious any day, but I guess that's not saying much.
[10:47:34] <jml> jelmer: I think a lot of LP's aesthetic issues are pretty shallow.
[10:48:38] <jelmer> jml: perhaps, if you'd keep the zopey bits in?
[10:49:31] <jelmer> jml: I wonder how much stuff is in there that is hard to write from scratch; it's mostly the views and overall behaviour that I like, personally.
[10:50:05] <lifeless> well, it will be orange soon
[10:50:08] <lifeless> that will help
[10:50:42] <jml> lifeless: didn't know there were plans to theme it properly
[10:51:59] <lifeless> jml: there is a 2 year old project to rebrand it for the current C theme
[10:52:11] <jml> jelmer: I'd like to hit some of the bug statuses with an axe of great hitting
[10:52:15] <lifeless> but the design workflow and resources are daft so little projress
[10:52:23] <jml> vaguely https://dev.launchpad.net/IssueTracker
```

```text
[10:52:28] <lifeless> yah
[10:52:53] <jml> heh.
[10:53:03] <lifeless> So, IMO, LP's biggest innovation bug tracker wise is - once setup - getting out of the way of devs nearly all the time
[10:53:19] <lifeless> the github issue tracker is a ghetto IME
[10:53:22] <jml> maybe I should knock something up in my coming week + half off, and write it in Haskell just for kicks.
[10:53:37] <jml> [I wouldn't, of course]
[10:53:58] * jelmer might be tempted to contribute to a project like that
[10:54:00] <jml> lifeless: can you elaborate on "getting out of the way".  I don't see how LP does that more than other trackers
[10:54:16] <lifeless> I'm not sure I can
[10:54:25] <jml> jelmer: hmm. clojure?
[10:54:26] <lifeless> it has some frustrations
[10:54:32] <lifeless> UI shallow stuff
[10:54:54] <jelmer> jml: I don't have experience with clojure yet; it was one of the things I was going to look at during my time off.
[10:54:55] <jml> Actually, I'm not sure I'd do clojure either. I'd just nick the ideas and write something that runs fast in Pypy. :\
[10:55:00] <jml> But clojure is fun.
[10:55:08] <jml> And I'd like to learn something about JVM programming.
[10:55:09] <lifeless> so, for a new bug tracker.
[10:55:11] <lifeless> I'd want...
[10:55:17] <jelmer> (In actuality, I haven't touched any code for months)
[10:55:22] * jml wants Maltesers
[10:55:26] <lifeless> API
[10:55:29] <jml> jelmer: I barely have either
[10:55:40] <lifeless> with callbacks (whatever sort - pshb, webhooks, don't care)
[10:55:56] <lifeless> I"m coding more now than I did @ Canonical in my last year
[10:56:01] <jml> lifeless: +1
[10:56:08] <lifeless> For more money with more prestige. Its great.
[10:56:36] <lifeless> Tho the money aspect is incidental
[10:56:41] <jml> coding leads to deployment; deployment leads to waiting; waiting leads to anger; anger leads to ...
[10:57:19] <lifeless> anyhow, API, callbacks. I want an evidence based model.
[10:57:30] <jml> lifeless: What do you mean by that?
[10:57:31] <jelmer> lifeless: nice
[10:57:38] <lifeless> 'I observed X' from users, 'Causes are A, B, C' from devs / experts.
[10:57:46] <jml> lifeless: Ah, yes.
[10:58:04] <lifeless> I don't want a freaking stack freaking exchange ghetto.
[10:58:12] <lifeless> Just in case that crossed your mind.
[10:58:42] <lifeless> I like the assume sensible behaviour default-open LP has. I'd like to keep that.
[10:58:51] <jml> Tech-wise, I'd want an immutable storage model. I think a lot of interesting information & decision making processes that LP/Ubuntu could have done have been hampered by throwing away data.
[10:59:19] <lifeless> jml: thats an interesting assertion, particularly given LP's obsession with not deleting.
:


[10:59:40] <jml> lifeless: yeah. I'm probing it with my mind-tongue as I type :)


[10:59:50] <lifeless> any blood?
[11:00:02] <jml> not that I can tell.
[11:00:24] <lifeless> I don't particularly care about the website but I do want one. And I want it snappy.
[11:00:51] <lifeless> I don't want to have to copy-paste anything ever related to code pushes/releases/...
[11:01:15] <lifeless> I don't care about release management - pypi / other language specific hosting site FOO will take care of that for me.
[11:01:43] <lifeless> I don't care about multiple long lived branches in the bug tracker model - it can have them, or not. Shrug.
[11:01:44] <jml> so, "fixed" means "fixed in trunk"?
[11:02:07] <lifeless> jml: Ah, I meant files and 'released on date Y' metadata.
[11:02:21] <lifeless> jml: I think 'in trunk' vs 'in a tarball' is still useful for many people.
[11:02:31] <jml> lifeless: yeah, I agree with that last.
[11:02:40] <lifeless> jml: but thats where teh API desire comes in.
[11:03:21] <jml> lifeless: I don't see the connection. … So that automated release tools can do roughly what testtools's release process does now?
[11:03:35] <lifeless> jml: I don't care about subcomponents and other highly refined metadata; if its big enough to need a structured marker, its big enough to be its own project.
[11:04:05] <lifeless> jml: yes, and so that push to (e.g.) github can fix-committed the bug.
[11:04:25] <lifeless> jml: both in-trunk and in-a-tarball are things I want to automate and forget.
[11:04:55] <lifeless> jml: this isn't a list of 'what LP doesn't do'
[11:05:06] <lifeless> jml: its a list of 'if I were to change my fav bugtracker, what would it look like'
[11:06:45] <jml> lifeless: understood
[11:07:17] <jml> lifeless: I'm not sure about components / tags
[11:07:20] <lifeless> Oh, and I'd like, for the few projects that do become popular, for the bugtracker to not assume that 'project priorities' == 'user priorities' == 'contributor priorities'
[11:07:48] <lifeless> *that* requires some careful drawing of boundaries by the designer, as the naive approach is fugly to use.
[11:07:56] <jml> Yeah. WONTFIX is not a valid bug status for open source projects.
[11:08:04] <jml> At least, not the way LP defines it.
[11:08:50] <lifeless> bugzilla, github, lp, debian - all make this mistake
[11:11:09] <lifeless> jml: I think there is a WONTFIX, in the sense that I can say "I will not merge a change that meets this use case ever."
[11:11:43] <jml> lifeless: yeah, agreed.
[11:11:47] <lifeless> federation is one answer to that tension
[11:11:56] <lifeless> 'oh yeah, well I am publishing a tree that does it'
[11:12:09] <lifeless> its not a very satisfying answer
[11:12:21] <jelmer> lifeless: isn't that something that's specific to a particular line of code?
[11:12:38] <jelmer> s/line of code/branch of the code/
[11:12:49] <jelmer> lifeless: much like a particular branch can contain a fix for a bug
[11:12:52] <lifeless> jelmer: maybe
[11:13:02] <lifeless> jelmer: but consider squid
[11:13:18] <lifeless> jelmer: we have code to MITM SSL connections in it
```

```text
[11:13:51] <lifeless> jelmer: if as a project we had said 'we consider this immoral and won't support it'
[11:14:05] <lifeless> jelmer: then it might be per branch, for-all project official branches
[11:14:25] <lifeless> jelmer: and perhaps even 'for all branches hosted on project servers'
[11:15:12] <jelmer> lifeless: that would make sense. Though it requires speaking about larger entities than just simple branches, which complicates things.
[11:16:03] <lifeless> jelmer: right
[11:16:24] <lifeless> jelmer: so I think that one of the complections LP did was to say 'bugs talk about code'
[11:16:28] <lifeless> or branches
[11:16:37] <lifeless> I think on reflection that that is false.
[11:16:59] <lifeless> bugs /may/ talk about a branch, or a project, or an effort
[11:17:06] <lifeless> its fluffy
[11:17:52] <lifeless> Is binding them together necessary to talk about WONTFIX. I don't think so.
[11:17:54] * jelmer thinks 'bug' is a confusing word in this context
[11:18:30] <lifeless> hah. 'bug reports may talk about a branch or project or effort'
[11:18:58] <lifeless> interesting actors in a bug report
[11:19:00] <lifeless> a user
[11:19:02] <lifeless> some users
[11:19:09] <lifeless> a volunteer
[11:19:13] <jelmer> issue tracker?
[11:19:13] <lifeless> some volunteers
[11:19:16] <lifeless> the project
[11:19:52] <jml> the loon
[11:19:58] <jml> the ideological purist
[11:20:00] <lifeless> the way folk get their knickers in a twist over bug report vs issue vs ... is a nuisance
[11:20:10] <jml> the f(o)under
[11:21:20] <jelmer> lifeless: not sure I agree; "bug" implies it's a problem in the code to me. Maybe I'm being pedantic.
[11:22:31] <lifeless> jml: hah, yes. Though I'd argue the f(o)under has sudo project privs.
[11:22:39] <jelmer> lifeless: considering the interested parties all have their own priorities, would you still explicitly want to track those priorities?
[11:22:54] <lifeless> jelmer: this is why I say some design work is needed.
[11:22:54] <jelmer> some?
[11:23:13] <jml> :)
[11:23:14] <lifeless> jelmer: :) :)
[11:23:32] <jml> abentley has often said that he'd want a priority list per person.
[11:23:47] <jml> interestingly, this somewhat clashes with the way I work for my non-bug todos
[11:23:59] <lifeless> jelmer: so, consider a project member. The project likely has some a-priori agenda (whether commercial like LP itself, or opinionated, like testtools)
[11:24:15] <jml> where I generally don't bother tracking priorities, but instead make it so I can easily retrieve a list of things to do for a particular context and/or goal
[11:24:31] <lifeless> jelmer: a project member may well want to track both personal interests and project interests.
```

```text
[11:24:50] <lifeless> [NB: I'm not strictly talking priorities here.]
[11:25:21] <lifeless> jelmer: now consider a user. They probably don't want to track anything, but they want to know if *anyone* has decided they will do this thing soon.
[11:25:48] <lifeless> jelmer: so they want [queue depth in days for this bug to be what someone is hacking on]
[11:26:50] <lifeless> jelmer: now consider the project again. Beyond or complementary to its a-priori agenda, it may well want to know what things are most frustrating to its userbase.
[11:27:34] <lifeless> jelmer: which is where tracking incident rates [for crashes] and reporting rates [for frequently requested dups], and 'me wants it too' etc data comes in.
[11:27:34] <jml> back
[11:27:53] <jml> on the 'context' thing, that's one reason I like tags.
[11:27:55] <lifeless> jelmer: so, do I want to track every users priority on every bug in every project... probably.
[11:28:05] <lifeless> jelmer: but do I want to show it as that? Hell no.
[11:28:19] <jml> because often it's easier to fix bugs in the same area
[11:28:24] <jml> or of the same type
[11:28:28] <lifeless> jml: yeah
[11:29:04] <lifeless> jelmer: so for instance, on openstack. It would be awesome if companies like HP that have their own priorities could just mark up the main bugtracker with all their desires
[11:29:13] <lifeless> without conflicting with 'the project' view of the world.
[11:29:41] <jelmer> lifeless: I like the idea
[11:29:41] <lifeless> jelmer: federation says 'HP should have their own private tracker' [which they do]
[11:30:03] <lifeless> jelmer: but its unsatisfying because unless you have bi-di sync you lose data
[11:30:19] <lifeless> and if you have bi-di sync then you have a base data model to do the sorts of things I express above
[11:31:02] <lifeless> Note that a few nice things fall out of this, if you don't consider showing everyone's values to everyone ever.
[11:31:22] <lifeless> the 'bug graffiti' of someone unprivileged closing a bug becomes more managable
[11:31:32] <jml> oh
[11:31:33] <jml> btw,
[11:31:47] <jml> hmm, that probably didn't need multiple newline emphasis.
[11:31:57] <jml> anyway, both of you should read "What Money Can't Buy"
[11:32:15] <lifeless>     [user says 'this is fix released' but you treat that as input to 'what users think' aggregate data rather than a change on the core.
[11:32:22] <lifeless> jml: thanks, will do.
[11:32:45] <lifeless> anyhow, rambling.
[11:33:06] <lifeless> If I was to hack on a bug tracker again, I think I'd do a pure local storage one
```

```text
[11:33:13] <lifeless> and let other folk do HTTP and HTML bindings.
[11:33:44] <lifeless> I have enough on my TODO without adding *another* web app.
[11:33:51] <jml> :)
[11:34:07] <lifeless> trackers are inherently social though, so it would be awkward :)
[11:34:22] <lifeless> I think I'd want a group of folk that will include folk interested in the upper layers.
[11:35:07] <jml> agreed
[11:35:17] <jml> I actually think I would consider paying someone to do the visuals


[11:35:50] <lifeless> well, if you do HTTP I can do the guts.
[11:36:15] <lifeless> gnight
[11:36:31] <jml> lifeless: g'night!
[11:36:44] <jml> (but I really, really want to try datomic!)
[11:37:18] <lifeless> jml: hah. I think datomic is only mildly interesting to me because of the amount of time I've spent in that space already with VCS storage.
[11:37:27] <jml> lifeless: right.
[11:37:42] <jml> lifeless: I want to try it to get a feel for it.
[11:37:57] <lifeless> jml: his complecting talk is vastly more interesting to me than a new language-coupled proprietary immutable-basis store.
[11:38:06] <jml> lifeless: +1
[11:38:27] * lifeless is gone
[11:40:42] <jml> OK, so what's the MVP
[11:40:55] <jml> and what's our success metric
[12:15:09] <jelmer> who are going to be early adopters? testtools?
[12:21:20] <jml> I guess.
```

## mbp says

Some of the Launchpad bug tracker and the Ubuntu crash tracker help of this, but I would like to see much more done in open source bug trackers towards this:

* default towards doing nothing.
* don't encourage developers to triage everything[b]
* bugs should decay from view
* still make it possible to dig out old bugs
* good automatic dupe-finding, including retrospectively or interactively hunting for dupes
* show an automatic estimate of the odds a bug will be fixed

## lifeless (LCA notes)

I spoke with stewart and devananda at LCA about this; they like the basic idea. Some things from them:

* releases and backports do matter
* very interested in being able to handle crashdumps.
* we discussed a bit of a data model that might capture this
* symptom (user, description), relates to symptom (dups), series (present in from $date, not present from $date), fix (addressed by)
* series (description), relates to symptom, fix (landed on $date), release (on date)
* fix (description), relates to symptom, series.

## jelmer (GSoC 2013)

Had a discussion with a handful of folks about distributed bug trackers at GSoC 2013. While there are a handful of distributed bug trackers out there, nobody has seen any that they like.

We agreed that code and bug tracking are very different things - as easy as it might seem to piggyback onto a DVCS somehow as a way to leverage an existing database:

* having the bug data in the main repository is noisy:
  * can require a significant amount of storage (especially if it includes attachments, etc)
  * lots of noise related to bugtracker in the commit history, making it less useful
  * lots of extra rebasing/merging to cope with bug commits
  * removing bugs/cleaning up spam requires rebasing the main code branch
* action on central repository required to report bug (e.g. merge request)
  * automatically merging is possible, but would require lots of hooks to prevent (non-developer) users from breaking format of files, truncating bugs, etc.
  * would need fancy merge hooks for bug data
* bugs are (generally) independent of each other, sharing history isn't particularly useful
* impossible to have security bugs in central bug tracker
* impossible to import to just some bugs without rewriting history

Some conclusions:

Distributed tracking is useful, but not everybody will want a copy of the bug database just to make changes to it - unlike e.g. with a distributed VCS. Some use cases:

* cooperation between distributions and upstream
* forks
* individual developers that want a local copy (for analysis, or offline use, etc)
* backup

Since most users will not be developers of the project, a web UI is mandatory; perhaps also email integration. Most users will likely end up using the web UI.

Eventual thoughts were that it might be useful to reuse some of the infrastructure from e.g. git but not store data in the main repository.

History should be kept per bug, so it possible/easier to:

* have just some bugs in the database and not others (in case bugs apply just to a fork, a branch, a downstream package, etc)
* have ACLs and e.g. hide security bugs from the public
* remove spammy bugs/edit out launch codes without affecting the rest of the bug database

Several people mentioned they would like the ability to interact with external bug trackers (e.g. github) as a way to have a local view on all their bugs in all their projects.
I think that's useful, but would like to avoid the complication of having to support the superset of all bug trackers. Perhaps just a way of importing data from external bug trackers is sufficient - it's important that it doesn't make the core more complex.

Martin Pool adds, "from andyfitz's talk, a hierarchy of needs for design: function> reliability > usability > pleasure"

[b]robertc:
I think triage as Launchpad does it now is useful, not as Ubuntu does it. [which is to say, encourage devs to look briefly at all reports to identify zomg things].

---

Martin Pool:
I think it makes some sense to look at every bug briefly to decide whether you're going to do anything or not.  It is also nice for users to get a definitive answer whether their bug will be worked or not.

But the cost per bug must be low.  That implies:

* project rules/habits for bug triage must allow a quick and (normally) final decision about whether to do any further work on the bug
* the information to do this should be on a single screen in the bug tracker
* expressing this in the bug tracker ought to be very quick (ideally one click or keypress to deal with the bug and move to the next, and of course <100ms load time)

If you spend substantial time investigating a bug in the ~90% of bugs that won't get fixed, that can add up to a substantial amount of wasted work.

---

robertc:
I do agree that you can easily create a lot of waste. I think its a mistake to focus too much on what devs need to do in assessing waste though: equally what users are doing by filing bugs is contributing effort too, and that should be respected much as we respect developer effort. When the supply of those things is asymmetric, we may need to limit either supply of work-for-devs, or effort by devs per bug.

---

Martin Pool:
Oh, I certainly agree this can waste user time - and I think that needs the
same thing.  Somehow communicate to users what kind of bug is very unlikely
to every be fixed, so they can avoid wasting their time filing it.  I think
tens of percent of bugs in Launchpad were a waste of time to even file.
