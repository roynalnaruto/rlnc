// =============================================================================
// RLNC for Ethereum Block Propagation — Typst Slides (touying + Metropolis)
// =============================================================================

#import "@preview/touying:0.6.3": *
#import "@preview/fletcher:0.5.8" as fletcher: diagram, node, edge
#import "@preview/cetz:0.3.4"

// --- Theme setup ---
#import themes.metropolis: *

#show: metropolis-theme.with(
  aspect-ratio: "16-9",
  config-info(
    title: [RLNC for Ethereum Block Propagation],
    subtitle: [Linearly-Homomorphic Signatures for Network Coding],
    author: [\[Author Name\]],
    institution: [\[Institution\]],
    date: datetime.today().display("[month repr:long] [day], [year]"),
  ),
  config-colors(
    primary: rgb("#23373b"),        // dark teal headers
    primary-light: rgb("#d9822b"),  // orange accents
    secondary: rgb("#d9822b"),
    neutral-lightest: white,
  ),
  config-common(
    slide-fn: slide,
  ),
  config-methods(
    init: (self: none, body) => {
      set text(size: 20pt)
      body
    },
  ),
)

// --- Math shorthands ---
#let Fp = $FF_p$
#let G1 = $GG_1$
#let G2 = $GG_2$
#let GT = $GG_T$
#let vv(x) = $bold(#x)$
#let MM(x) = $bold(#x)$
#let MSM = math.op("MSM")

// --- Layout helpers ---
#let keybox(body) = {
  block(
    width: 100%,
    inset: 12pt,
    radius: 4pt,
    fill: rgb("#d9822b").lighten(80%),
    stroke: 2pt + rgb("#d9822b"),
    text(weight: "bold", body),
  )
}

#let styledtable(..args) = {
  set table(
    stroke: 0.5pt + luma(180),
    inset: 6pt,
    fill: (_, y) => if y == 0 { rgb("#23373b").lighten(85%) },
  )
  table(..args)
}


// =============================================================================
// TITLE SLIDE
// =============================================================================

#title-slide()

// =============================================================================
// PART I — THE PROBLEM
// =============================================================================

= The Problem

// --- I.a: Ethereum's Roadmap → Bigger Blocks ---
== Ethereum's Roadmap: Bigger Blocks

#slide[
  #grid(
    columns: (1fr, 1fr),
    gutter: 20pt,
    [
      - Rollup-centric roadmap: L2s handle transactions,
        L1 provides *settlement + data availability*

      - Each upgrade ships more blobs and raises gas limits

      - Three compounding forces:
        + *More blobs* (6 → 128+)
        + *Higher gas limits* (60M → 200M+)
        + *zkEVM* removes re-execution burden

      - Core tension: the p2p layer was built for blocks
        an order of magnitude smaller
    ],
    [
      #align(center)[
        #styledtable(
          columns: 3,
          table.header([*Era*], [*Blobs*], [*Data*]),
          [Today (post-Fusaka)], [14], [~2 MB],
          [Glamsterdam], [48], [~6.5 MB],
          [Hegota], [72], [~10 MB],
          [zkEVM], [128+], [~17+ MB],
        )
      ]
    ],
  )
]

// --- I.b: Gossipsub — How Blocks Travel Today ---
== Gossipsub: How Blocks Travel Today

#slide[
  #grid(
    columns: (1fr, 1fr),
    gutter: 20pt,
    [
      - Every node maintains a *mesh* of $D approx 8$ peers

      - *Full-block forwarding*: a node receives the _entire_
        block before forwarding to all mesh peers

      - *Redundant delivery*: mesh connections overlap —
        nodes frequently receive duplicates

      - Hops to reach all $n$ nodes:

      $ k = ceil(frac(ln n, ln D)) approx 5 dash 6 "hops" $
    ],
    [
      #align(center)[
        #diagram(
          spacing: (18pt, 26pt),
          node-stroke: 1pt,
          {
            let nsize = 9pt

            // Proposer
            node((1.5, 0), [Proposer], fill: rgb("#e74c3c"), stroke: rgb("#c0392b"),
                 corner-radius: 4pt, name: <p>)

            // Hop 1: Peer_1, Peer_2, ..., Peer_D
            node((0, 1), text(nsize)[$"Peer"_1$], fill: rgb("#f39c12"),
                 stroke: rgb("#d68910"), corner-radius: 4pt, name: <a1>)
            node((1, 1), text(nsize)[$"Peer"_2$], fill: rgb("#f39c12"),
                 stroke: rgb("#d68910"), corner-radius: 4pt, name: <a2>)
            node((2, 1), text(nsize)[$dots.c$], stroke: none, name: <adots>)
            node((3, 1), text(nsize)[$"Peer"_D$], fill: rgb("#f39c12"),
                 stroke: rgb("#d68910"), corner-radius: 4pt, name: <aD>)

            for name in (<a1>, <a2>, <adots>, <aD>) {
              edge(<p>, name, "->")
            }

            // Hop 2: D peers shown under Peer_1, shifted left to centre
            node((-1, 2), text(nsize)[$"Peer"_1$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <b1>)
            node((0, 2), text(nsize)[$"Peer"_2$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <b2>)
            node((1, 2), text(nsize)[$dots.c$], stroke: none, name: <bdots>)
            node((2, 2), text(nsize)[$"Peer"_D$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <bD>)

            for name in (<b1>, <b2>, <bdots>, <bD>) {
              edge(<a1>, name, "->")
            }

            // Hop labels
            node((4, 0), text(9pt, fill: luma(120))[Hop 0], stroke: none)
            node((4, 1), text(9pt, fill: luma(120))[Hop 1], stroke: none)
            node((4, 2), text(9pt, fill: luma(120))[Hop 2], stroke: none)
          },
        )
      ]
    ],
  )
]

// --- I.c: The Scalability Wall ---
== The Scalability Wall

#slide[
  Propagation time over $k$ hops:

  $ T_"gossipsub" = k dot (L + B / X) $

  #pause

  #grid(
    columns: (1fr, 1fr, 1fr),
    gutter: 12pt,
    [
      *Today* ($B = 2$ MB):
      $ T = 6 times 170 = 1020 "ms" $
    ],
    [
      *Hegota* ($B = 10$ MB):
      $ T = 6 times 570 = 3420 "ms" $
    ],
    [
      *zkEVM* ($B = 17$ MB):
      $ T = 6 times 920 = 5520 "ms" $
    ],
  )

  #pause

  Maximum block size within a 4 s budget:

  $ B_max = T_"budget" / k dot X - L dot X approx 11.7 "MB theoretical" arrow.r 5 dash 6 "MB practical" $

  #keybox[
    Gossipsub cannot sustain the block sizes Ethereum's roadmap demands.
  ]
]

// --- I.d: Focus ---
#focus-slide[Random Linear Network Coding]


// =============================================================================
// PART II — NETWORK CODING
// =============================================================================

= Network Coding

// --- II.a: Mixing Instead of Copying ---

// Color helpers for the paint analogy
#let cRed = rgb("#c0392b")
#let cGreen = rgb("#27ae60")
#let cBlue = rgb("#2980b9")
#let mR(x) = text(fill: cRed)[$#x$]
#let mG(x) = text(fill: cGreen)[$#x$]
#let mB(x) = text(fill: cBlue)[$#x$]

== Mixing Instead of Copying

#slide[
  Alice has a recipe: $(#mR[5], #mG[3], #mB[7])$ — grams of
  #text(fill: cRed)[*Red*], #text(fill: cGreen)[*Green*], #text(fill: cBlue)[*Blue*].

  Instead of sending pure pigments, she sends *mixtures* with labels:

  #align(center)[
    #styledtable(
      columns: 4,
      table.header([*Bucket*], [*Label (ratios)*], [*Contents*], [*Total*]),
      [To Bob], [$(2, 1, 0)$],
        [$2 times #mR[5] + 1 times #mG[3] + 0 times #mB[7]$], [$13$],
      [To Carol], [$(0, 1, 3)$],
        [$0 times #mR[5] + 1 times #mG[3] + 3 times #mB[7]$], [$24$],
      [To Dave], [$(1, 0, 2)$],
        [$1 times #mR[5] + 0 times #mG[3] + 2 times #mB[7]$], [$19$],
      [To Eve], [$(3, 2, 1)$],
        [$3 times #mR[5] + 2 times #mG[3] + 1 times #mB[7]$], [$28$],
    )
  ]
]

== Mixing Instead of Copying (cont.)

#slide(repeat: 3, self => [
  #let (uncover, only) = utils.methods(self)

  Dave's bucket is lost! Solve from Bob, Carol, Eve:

  $ 2#mR[R] + 1#mG[G] + 0#mB[B] &= 13 \
    0#mR[R] + 1#mG[G] + 3#mB[B] &= 24 \
    3#mR[R] + 2#mG[G] + 1#mB[B] &= 28 $

  #uncover("2-")[
    - From Bob: $#mG[G] = 13 - 2#mR[R]$
    - Substitute into Carol: $3#mB[B] = 11 + 2#mR[R]$
    - Substitute into Eve → $#mR[R] = #mR[5]$,
      $#mG[G] = #mG[3]$, $#mB[B] = #mB[7]$
  ]

  #uncover("3-")[
    *Anyone can remix:* Bob combines his + Carol's → valid mixture
    $(2, 2, 3)$, value $37$. No original pigments needed!

    #set text(size: 16pt)
    #table(
      columns: 2,
      stroke: 0.5pt + luma(180),
      inset: 4pt,
      fill: (_, y) => if y == 0 { rgb("#23373b").lighten(85%) },
      table.header([*Paint*], [*RLNC*]),
      [Recipe $(#mR[5], #mG[3], #mB[7])$], [Block → $N$ chunks],
      [Pigment / Bucket], [Chunk $vv(v)_i$ / Coded chunk $vv(w)$],
      [Label (ratios)], [Coefficient vector $vv(b)$],
      [Solve equations], [Gaussian elimination over #Fp],
    )
  ]
])

// --- II.b: Vectors, Subspaces & Linear Independence ---
== Vectors, Subspaces & Linear Independence

#slide[
  A block is $N$ vectors in $Fp^M$: #h(1em) $vv(v)_1, vv(v)_2, dots, vv(v)_N$

  A *linear combination*:
  $ vv(w) = sum_(i=1)^N b_i dot vv(v)_i $

  #keybox[Need $N$ linearly independent coded chunks to decode.]

  #v(8pt)

  *Why random coefficients work:*

  $ Pr["dependent"] <= 1/p approx 2^(-252) $

  With $b_i$ sampled uniformly from #Fp, every random combination is
  independent with overwhelming probability.
]

// --- II.c: RLNC Encoding ---
== RLNC Encoding

#slide[
  #grid(
    columns: (1fr, 1fr),
    gutter: 20pt,
    [
      The proposer holds $vv(v)_1, dots, vv(v)_N$.

      For each peer $j$:
      + Sample random $vv(b)^((j)) arrow.l.squiggly Fp^N$
      + Compute coded chunk:
        $ vv(w)^((j)) = sum_(i=1)^N b_i^((j)) dot vv(v)_i $
      + Send $(vv(w)^((j)), vv(b)^((j)))$

      Each peer gets a *different* random linear combination.
    ],
    [
      #align(center)[
        #diagram(
          spacing: (22pt, 36pt),
          node-stroke: 1pt,
          {
            let nsize = 11pt
            node((1.5, 0), text(nsize)[Proposer], fill: rgb("#e74c3c"),
                 stroke: rgb("#c0392b"), corner-radius: 4pt, name: <prop>)

            node((0, 1), text(nsize)[$"Peer"_1$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <p1>)
            node((1, 1), text(nsize)[$"Peer"_2$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <p2>)
            node((2, 1), text(nsize)[$dots.c$], stroke: none, name: <pdots>)
            node((3, 1), text(nsize)[$"Peer"_D$], fill: rgb("#3498db"),
                 stroke: rgb("#2980b9"), corner-radius: 4pt, name: <pD>)

            edge(<prop>, <p1>, "->", label: text(10pt)[$vv(w)^1$], label-side: right)
            edge(<prop>, <p2>, "->", label: text(10pt)[$vv(w)^2$], label-side: left)
            edge(<prop>, <pdots>, "->")
            edge(<prop>, <pD>, "->", label: text(10pt)[$vv(w)^D$], label-side: left)
          },
        )
      ]
    ],
  )
]

// --- II.d: Re-encoding & Decoding ---
== Re-encoding & Decoding

#slide[
  *Re-encoding* (at intermediate nodes):

  Given $L$ received coded chunks, sample $alpha_1, dots, alpha_L$:

  $ vv(w)' = sum_(ell=1)^L alpha_ell dot vv(w)_ell, #h(2em)
    b'_i = sum_(ell=1)^L alpha_ell dot b_(ell, i) $

  Intermediate nodes operate *entirely on coded chunks* — no original data needed.

  #pause

  *Decoding* (once $N$ independent chunks collected):

  $ mat(delim: "(", vv(w)_1; dots.v; vv(w)_N) = MM(B) dot
    mat(delim: "(", vv(v)_1; dots.v; vv(v)_N) #h(2em) arrow.r #h(2em)
    mat(delim: "(", vv(v)_1; dots.v; vv(v)_N) = MM(B)^(-1) dot
    mat(delim: "(", vv(w)_1; dots.v; vv(w)_N) $
]

// --- II.e: The Integrity Problem ---
== The Integrity Problem

#slide[
  *Pollution attack:* a malicious node injects a garbage coded chunk.

  - The garbage is linearly combined with legitimate chunks at every
    downstream node
  - When a node collects $N$ chunks and inverts the matrix, it recovers
    *the wrong block*
  - The corruption is *invisible* until decoding — too late

  #v(12pt)

  Verification is hard: receivers only see coded chunks, never the originals.

  #keybox[How does a receiver know a coded chunk is legitimate?]
]

// --- II.f: Pedersen Commitments ---
== Pedersen Commitments

#slide[
  Let $G_1, G_2, dots, G_M in GG_1$ be public generators on BLS12-381.

  *Construction* — commit to vector $vv(v) = (a_1, dots, a_M)$:

  $ C(vv(v)) = sum_(j=1)^M a_j dot G_j #h(1em) in GG_1 $

  A single #G1 point — 48 bytes compressed.

  #pause

  *Homomorphic property:*

  $ C(alpha vv(v) + beta vv(u)) = alpha dot C(vv(v)) + beta dot C(vv(u)) $

  Linearity of scalar multiplication gives us this for free. A commitment to a
  coded chunk equals the same linear combination of the original commitments.
]

// --- II.g: Pedersen for RLNC — Verify Pipeline ---
== Pedersen for RLNC: Verification

#slide[
  Proposer computes $C_i = C(vv(v)_i)$ for each chunk and signs
  $(C_1, dots, C_N)$ with BLS → $sigma$.

  Each packet carries: #h(1em) $(vv(w), vv(b), {C_1 dots C_N}, sigma)$

  #v(6pt)

  *3-step verification pipeline:*

  #grid(
    columns: (auto, 1fr),
    column-gutter: 8pt,
    row-gutter: 20pt,
    text(fill: rgb("#d9822b"), weight: "bold")[1.],
    [*BLS signature check* — verify $sigma$ on $(C_1, dots, C_N)$ with proposer's pk],
    text(fill: rgb("#d9822b"), weight: "bold")[2.],
    [*Commitment check* — verify $C(vv(w)) attach(=, t: ?) sum_(i=1)^N b_i dot C_i$],
    text(fill: rgb("#d9822b"), weight: "bold")[3.],
    [*Independence check* — is $vv(b)$ linearly independent of previously received vectors?],
  )

  #v(6pt)
  If any check fails → discard. Pollution attacks detected and rejected.
]

// --- II.h: The Commitment Overhead Problem ---
== The Commitment Overhead Problem

#slide[
  #align(center)[
    #styledtable(
      columns: 3,
      inset: (x: 10pt, y: 12pt),
      table.header([*Component*], [*Size*], [*Per packet?*]),
      [$vv(w)$ (coded data)], [$32M$ bytes], [Unique],
      [$vv(b)$ (coefficients)], [$32N$ bytes], [Unique],
      [$C_1 dots C_N$ (commitments)], [$48N$ bytes], [*Identical*],
      [$sigma$ (BLS signature)], [$96$ bytes], [*Identical*],
    )
  ]

  $N$ commitments repeated in *every* packet at *every* hop.

  #grid(
    columns: (1fr, 1fr),
    gutter: 16pt,
    [
      $N = 10$: #h(1em) *128 MB* redundant traffic
    ],
    [
      $N = 64$: #h(1em) *819 MB* redundant traffic
    ],
  )

  #keybox[What if we could replace $N$ commitments with a single signature?]
]


// =============================================================================
// PART III — LINEARLY-HOMOMORPHIC SIGNATURES
// =============================================================================

= Linearly-Homomorphic Signatures

// --- III.a: Bilinear Pairings ---
== Bilinear Pairings

#slide[
  A *bilinear pairing* maps pairs of curve points to a target group:

  $ e : G1 times G2 arrow.r GT $

  *Bilinearity:* #h(1em) $e(a dot P, b dot Q) = e(P, Q)^(a b)$

  #v(6pt)

  #grid(
    columns: (1fr, 1fr),
    gutter: 20pt,
    [
      #styledtable(
        columns: 2,
        table.header([*Group*], [*Size (BLS12-381)*]),
        [#G1], [48 bytes],
        [#G2], [96 bytes],
        [#GT], [576 bytes],
      )
    ],
    [
      *BLS signature* (motivating example):
      - $sigma = italic("sk") dot H(m) in G2$
      - Verify: $e(G_1, sigma) attach(=, t: ?) e(italic("pk"), H(m))$
      - Works because bilinearity moves
        $italic("sk")$ across arguments
    ],
  )

  #v(4pt)
  *Key insight:* pairings let you check scalar relationships between
  secret keys and signed data *without revealing the keys*.
]

// --- III.b: LHSS Intuition — "Certified Paint" ---
== LHSS Intuition: Certified Paint

#slide[
  Callback to Alice's paint analogy:

  - The proposer *certifies* each base pigment with a cryptographic stamp ($sigma_i$)

  - Anyone can blend certified pigments and *derive* a valid stamp for the blend
    — without the proposer's stamp pad (secret key)

  - But you *cannot* certify a color not mixed from the originals (security)

  #v(8pt)

  Three formal properties:

  + *Verifiable* with public key only
  + *Composable* — combine $sigma_1, sigma_2$ into $sigma'$ without $italic("sk")$
  + *Controlled* — only within the linear span of signed vectors

  #keybox[Combine signatures WITHOUT the secret key.]
]

// --- III.c: LHSS Abstraction ---
== LHSS: The Four Algorithms

#slide[
  #grid(
    columns: (1fr, 1fr),
    gutter: 16pt,
    [
      $op("Setup")(1^lambda, M, N)$
      - → $(italic("sk"), italic("pk"))$

      #v(6pt)

      $op("Sign")(italic("sk"), italic("id"), vv(m), i)$
      - → signature $sigma$
    ],
    [
      $op("Combine")(italic("pk"), italic("id"), {(a_i, sigma_i)})$
      - → combined $sigma$ #text(fill: rgb("#d9822b"))[(public key only!)]

      #v(6pt)

      $op("Verify")(italic("pk"), italic("id"), vv(v), sigma, vv(a))$
      - → accept / reject
    ],
  )

  #v(8pt)

  *Correctness:*
  + Direct signatures verify: $op("Sign") arrow.r op("Verify") = 1$
  + Combined signatures verify:
    $ op("Verify")(italic("pk"), italic("id"), sum_i a_i vv(m)_i, op("Combine")(dots), sum_i a_i vv(a)_i) = 1 $
]

// --- III.d: BFKW Construction — Setup & Sign ---
== BFKW Construction: Setup & Sign

#slide[
  *Basis vector trick:* augment each chunk before signing

  $ vv(m)'_i = (vv(v)_i bar.v.double vv(e)_i) in Fp^(M+N) $

  Linear combination yields:
  #h(1em) $sum b_i dot vv(m)'_i = (vv(w) bar.v.double vv(b))$ — data *and* coefficients bound together.

  #pause

  *Setup:*
  - $italic("sk") arrow.l.squiggly Fp$, #h(1em) $italic("pk") = italic("sk") dot G_1 in G1$

  *Sign* chunk $vv(v)_i$ with index $i$:

  + Compute hash points: $H_s = H(italic("id"), s) in G2$ for $s = 1, dots, M+N$
  + $ P = sum_(s=1)^(M+N) m'_s dot H_s = MSM(H, [vv(v)_i bar.v.double vv(e)_i]) in G2 $
  + $ sigma = italic("sk") dot P in G2 $
]

// --- III.e: BFKW Construction — Combine & Verify ---
== BFKW Construction: Combine & Verify

#slide(repeat: 2, self => [
  #let (uncover, only) = utils.methods(self)

  *Combine* — MSM in #G2:

  $ sigma' = sum_(i=1)^k a_i dot sigma_i in G2 $

  *Correctness:* #h(1em)
  $sigma' = sum a_i dot italic("sk") dot MSM(H, vv(m)'_i)
          = italic("sk") dot MSM(H, sum a_i dot vv(m)'_i)
          = italic("sk") dot MSM(H, (vv(w) bar.v.double vv(b)))$

  #uncover("2-")[
    *Verify:* compute $P = MSM(H, [vv(w) bar.v.double vv(b)])$ and check:

    $ e(G_1, sigma) attach(=, t: ?) e(italic("pk"), P) $

    Bilinearity moves $italic("sk")$ from #G2 to #G1:

    $ e(G_1, italic("sk") dot P) = e(italic("sk") dot G_1, P) = e(italic("pk"), P) #h(1em) checkmark $
  ]
])

// --- III.f: Pedersen vs BFKW — Final Comparison ---
== Pedersen vs BFKW: Final Comparison

#slide[
  #grid(
    columns: (1fr, 1fr),
    column-gutter: 16pt,
    row-gutter: 6pt,
    align: top,
    // Row 1: packet diagrams
    [
      *Pedersen packet:*
      #block(
        width: 100%, inset: 6pt, radius: 4pt,
        stroke: 1pt + luma(180),
      )[
        #stack(
          dir: ttb, spacing: 2pt,
          rect(width: 100%, height: 20pt, fill: rgb("#3498db").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$vv(w)$ — coded data ($32M$ B)])],
          rect(width: 100%, height: 16pt, fill: rgb("#2ecc71").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$vv(b)$ — coefficients ($32N$ B)])],
          rect(width: 100%, height: 20pt, fill: rgb("#e74c3c").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$C_1 dots C_N$ — commitments ($48N$ B)])],
          rect(width: 100%, height: 14pt, fill: rgb("#f39c12").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$sigma$ — BLS sig (96 B)])],
        )
      ]
    ],
    [
      *BFKW packet:*
      #block(
        width: 100%, inset: 6pt, radius: 4pt,
        stroke: 1pt + luma(180),
      )[
        #stack(
          dir: ttb, spacing: 2pt,
          rect(width: 100%, height: 20pt, fill: rgb("#3498db").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$vv(w)$ — coded data ($32M$ B)])],
          rect(width: 100%, height: 16pt, fill: rgb("#2ecc71").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$vv(b)$ — coefficients ($32N$ B)])],
          rect(width: 100%, height: 14pt, fill: rgb("#9b59b6").lighten(40%),
               stroke: none)[#align(center, text(9pt)[$sigma$ — BFKW sig (96 B)])],
        )
      ]
    ],
    // Row 2: overhead (aligned)
    [Overhead: $80N + 96$ bytes],
    [Overhead: $32N + 96$ bytes],
  )

  #v(2pt)
  #align(center)[
    #set text(size: 16pt)
    #styledtable(
      columns: 4,
      inset: (x: 4pt, y: 6pt),
      table.header([*Metric*], [*Pedersen*], [*BFKW*], [*Winner*]),
      [Comm / packet], [$80N + 96$ B], [$32N + 96$ B], [*BFKW*],
      [Network overhead], [$cal(O)(n D N)$], [$cal(O)(n D)$], [*BFKW*],
      [Proposer compute], [$N$ $M$-MSMs in #G1], [$N$ $M$-MSMs + $D$ $N$-MSMs in #G2], [Pedersen],
      [Receiver compute], [2 MSMs], [1 MSM + 1 multi-pairing], [Comparable],
      [Setup], [Trusted generators], [Key pair only], [*BFKW*],
    )
  ]
  #keybox[LHSS wins on communication at the cost of slightly more computation.]
]


// =============================================================================
// CLOSING
// =============================================================================

#focus-slide[Thank You]
