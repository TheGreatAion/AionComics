<script lang="ts">
  import { api, paragraphs, storyTitle, type CvAbout, type PeopleKind, type PeopleView } from "$lib/api";
  import { lib, nav, openReader } from "$lib/state.svelte";
  import Cover from "$lib/components/Cover.svelte";

  // Everything in the library by one creator, with one character or team in it, or in one story arc,
  // one row of books per series, the series they're most in first.
  let { kind, value }: { kind: PeopleKind; value: string } = $props();
  let data = $state<PeopleView | null>(null);

  $effect(() => {
    void lib.version;
    const [k, v] = [kind, value];
    api.people(k, v).then((d) => {
      if (k !== kind || v !== value) return;
      data = d;
      nav.restore();
    });
  });

  const seriesCount = $derived(data?.series.length ?? 0);
  // What a creator did: "Pencils 412 · Cover 30 · Inks 2". (For a character it'd only repeat the count.)
  const roles = $derived(
    kind === "creator"
      ? (data?.roles ?? []).map(([role, n]) => `${role[0].toUpperCase()}${role.slice(1)} ${n.toLocaleString()}`).join(" · ")
      : "",
  );
  const first = $derived(data?.series[0]?.comics[0]);

  // What Comic Vine says about them: a picture, a line, a few facts and their story. Asked for once
  // and kept, and only when there's a Comic Vine key; nothing shows until it's here.
  let cv = $state<CvAbout | null>(null);
  let more = $state(false);
  $effect(() => {
    const [k, v] = [kind, value];
    cv = null;
    more = false;
    if (k === "arc") return;
    api
      .cvAbout(k, v)
      .then((a) => {
        if (k === kind && v === value) cv = a;
      })
      .catch(() => {});
  });
  const story = $derived(paragraphs(cv?.description ?? null, 12));
  const year = (d: string | null) => d?.slice(0, 4) ?? null;
  const month = (d: string | null) => (d ? new Date(d.slice(0, 10) + "T12:00").toLocaleDateString([], { month: "long", year: "numeric" }) : "");
  const facts = $derived.by(() => {
    if (!cv) return [];
    const f: [string, string][] = [];
    if (kind === "character") {
      if (cv.realName) f.push(["Real name", cv.realName]);
      if (cv.aliases.length) f.push(["Also known as", cv.aliases.slice(0, 3).join(", ")]);
      if (cv.origin) f.push(["Kind", cv.origin]);
      if (cv.issues) f.push(["In", `${cv.issues.toLocaleString()} issues`]);
      if (cv.creators.length) f.push(["Created by", cv.creators.slice(0, 3).join(", ")]);
    } else {
      if (cv.country) f.push(["From", cv.hometown ? `${cv.hometown}, ${cv.country}` : cv.country]);
      if (cv.born) f.push(["Born", year(cv.born)!]);
      if (cv.died) f.push(["Died", year(cv.died)!]);
    }
    return f;
  });
</script>

{#if data}
  <div class="page">
    <header class:rich={!!cv?.image}>
      {#if cv?.image}
        <img class="portrait" class:photo={kind === "creator"} src={cv.image} alt="" onerror={(e) => (e.currentTarget as HTMLElement).remove()} />
      {/if}
      <div class="who">
        <div class="kind">{{ creator: "Creator", character: "Character", arc: "Story arc" }[kind]}</div>
        <h1>{data.name}</h1>
        <div class="sum fine">
          {data.books.toLocaleString()} {data.books === 1 ? "book" : "books"} · {seriesCount.toLocaleString()} series{#if roles}&ensp;·&ensp;{roles}{/if}
        </div>
        {#if cv}
          <!-- A line that's only a word or two ("Writer.") says nothing the page doesn't. -->
          {#if cv.deck && cv.deck.length > 20}<p class="deck">{cv.deck}</p>{/if}
          {#if facts.length || cv.first}
            <div class="facts">
              {#each facts as [k, v] (k)}<span class="fact"><span class="k fine">{k}</span><span class="v">{v}</span></span>{/each}
              {#if cv.first}
                {@const f = cv.first}
                <span class="fact">
                  <span class="k fine">First appeared</span>
                  {#if f.comicId && f.seriesId}
                    <button class="v link" title="Your copy" onclick={() => nav.go({ name: "series", id: f.seriesId!, book: f.comicId! })}>{f.label}</button>
                  {:else}
                    <span class="v">{f.label}{#if f.date}<span class="dim"> · {month(f.date)}</span>{/if}</span>
                  {/if}
                </span>
              {/if}
            </div>
          {/if}
          {#each more ? story : story.slice(0, 1) as p, k (k)}<p class="bio">{p}</p>{/each}
          {#if story.length > 1 || (story[0]?.length ?? 0) > 360}
            <button class="link more" onclick={() => (more = !more)}>{more ? "Less" : "More"}</button>
          {/if}
        {/if}
      </div>
    </header>

    {#if !data.books}
      <p class="none">No books in your library list {data.name} yet.</p>
    {/if}

    {#each data.series as s (s.id)}
      <section>
        <div class="head">
          <button class="name" onclick={() => nav.go({ name: "series", id: s.id })}>{s.name}</button>
          <span class="fine dim">{s.collectionName} · {s.comics.length}</span>
        </div>
        <div class="row">
          {#each s.comics as c (c.id)}
            {@const t = storyTitle(c)}
            <button
              class="pick"
              data-nav
              class:done={c.read}
              title="{c.label}{t ? ` · ${t}` : ''}"
              onclick={() => nav.go({ name: "series", id: s.id, book: c.id })}
              ondblclick={() => openReader(c.id)}
            >
              <Cover id={c.id} ratio={c.coverRatio} width={92} pages={c.numPages} />
              <span class="cap fine">{c.label}</span>
            </button>
          {/each}
        </div>
      </section>
    {/each}
  </div>
{/if}

<style>
  .page {
    position: relative;
    isolation: isolate;
    padding: 120px 0 140px;
  }
  header {
    padding: 0 var(--pad);
  }
  /* With a picture from Comic Vine: it beside the name, their story under it. */
  header.rich {
    display: grid;
    grid-template-columns: 180px minmax(0, 1fr);
    gap: 40px;
    align-items: start;
  }
  .portrait {
    width: 180px;
    height: 180px;
    border-radius: 50%;
    object-fit: cover;
    object-position: top;
    box-shadow: 0 0 0 1px var(--line);
  }
  .portrait.photo {
    height: 225px;
    border-radius: 6px;
  }
  .who {
    min-width: 0;
    max-width: 880px;
  }
  .deck {
    margin-top: 18px;
    font-size: 16px;
    line-height: 1.5;
    color: var(--text);
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 14px 28px;
    margin-top: 18px;
  }
  .fact {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .fact .k {
    color: var(--text-3);
  }
  .fact .v {
    font-size: 14px;
    color: var(--text);
    text-align: left;
  }
  .fact .dim {
    color: var(--text-3);
  }
  .bio {
    margin-top: 14px;
    font-size: 14px;
    line-height: 1.6;
    color: var(--text-2);
    user-select: text;
  }
  .more {
    margin-top: 8px;
    font-size: 13px;
  }
  /* A phone: the picture above the name. */
  @media (max-width: 700px) {
    header.rich {
      grid-template-columns: minmax(0, 1fr);
      gap: 20px;
    }
    .portrait {
      width: 120px;
      height: 120px;
    }
    .portrait.photo {
      height: 150px;
    }
  }
  .kind {
    font-size: 13px;
    color: var(--text-2);
  }
  h1 {
    margin-top: 10px;
    font-family: var(--display);
    font-weight: 400;
    font-size: clamp(64px, 8vw, 120px);
    line-height: 0.88;
    letter-spacing: 0.005em;
    overflow-wrap: anywhere;
  }
  .sum {
    margin-top: 16px;
    color: var(--text-2);
  }
  .none {
    padding: 48px var(--pad) 0;
    color: var(--text-2);
  }
  section {
    margin-top: 56px;
  }
  .head {
    padding: 0 var(--pad);
    display: flex;
    align-items: baseline;
    gap: 14px;
    margin-bottom: 14px;
  }
  .name {
    font-family: var(--display);
    font-size: 30px;
    line-height: 1;
    letter-spacing: 0.01em;
    color: var(--text);
  }
  .name:hover {
    color: var(--strong);
  }
  .dim {
    color: var(--text-3);
  }
  /* A long run scrolls sideways rather than stretching the page. */
  .row {
    display: flex;
    gap: 22px;
    overflow-x: auto;
    padding: 10px var(--pad) 8px;
    align-items: flex-end;
  }
  .pick {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
  }
  .pick :global(.cover) {
    box-shadow: 0 16px 34px -16px rgba(0, 0, 0, 0.95);
    transition:
      transform 0.45s var(--ease),
      opacity 0.25s;
  }
  .pick.done :global(.cover) {
    opacity: 0.6;
  }
  .pick:hover :global(.cover) {
    transform: translateY(-5px);
    opacity: 1;
  }
  .cap {
    margin-top: 8px;
    font-size: 12px;
    color: var(--text-3);
  }
</style>
