---
name: Test Store
description: A focused account-to-test-payment flow, presented as a numbered record catalogue.
colors:
  amber: "#e7a13a"
  teal: "#78c5b7"
  ink: "#101216"
  paper: "#f4f0e6"
  muted-paper: "#c5c1b8"
  rule: "#c9c4b8"
  field-white: "#fffefa"
  deep-amber: "#80500f"
  deep-teal: "#317568"
  record-surface: "#17191d"
typography:
  headline:
    fontFamily: '"IBM Plex Sans", "Segoe UI", Arial, sans-serif'
    fontSize: "clamp(42px, 5vw, 58px)"
    fontWeight: 700
    lineHeight: 1.04
    letterSpacing: "-.045em"
  title:
    fontFamily: '"IBM Plex Sans", "Segoe UI", Arial, sans-serif'
    fontSize: "26px"
    lineHeight: 1.15
  body:
    fontFamily: '"Segoe UI", Arial, sans-serif'
    fontSize: "16px"
    lineHeight: 1.5
  label:
    fontFamily: '"Segoe UI", Arial, sans-serif'
    fontSize: "11px"
    fontWeight: 750
    letterSpacing: ".17em"
rounded:
  square: "0px"
spacing:
  control-inset: "12px"
  form-rhythm: "16px"
  panel-inset: "30px"
  record-inset: "22px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.square}"
    padding: "12px 18px"
    height: "48px"
  button-primary-hover:
    backgroundColor: "{colors.amber}"
    textColor: "{colors.ink}"
  field:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.square}"
    padding: "0 12px"
    height: "44px"
  amount-field:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.square}"
    padding: "0 12px"
    height: "46px"
  record-card:
    backgroundColor: "{colors.record-surface}"
    textColor: "{colors.paper}"
    rounded: "{rounded.square}"
    padding: "22px"
---

# Design System: Test Store

## Overview

**Creative North Star: "The Numbered Record Catalogue"**

The Numbered Record Catalogue treats an account, the single test item, and its payment as precise records. Matte ink and warm paper establish the working surface; orange and teal act as printed marks, while compact codes keep the sequence legible. The result is a focused utility, not a general merchandise storefront.

Typography and controls stay conventional and task-first: compact uppercase record labels contrast with IBM Plex Sans headings, paper panels sit squarely against the dark shell, and prices use tabular numerals. The display face is bundled locally; Segoe UI remains the body and control face. The account and confirmation flow leads to PRODUCTS, then a separate Stripe test checkout with an editable amount.

**Key Characteristics:**
- Ink-dark shell with warm-paper work surfaces.
- Amber and teal record markings, compact codes, and tabular price figures.
- IBM Plex Sans headings over familiar Segoe UI controls and body copy.
- Square, bordered panels and familiar web form controls.

## Colors

The palette pairs matte near-black with warm paper; amber and teal mark records and actions, with deeper accent shades used on paper for readable contrast.

### Primary
- **Printed Amber**: The main accent for record codes, test status, primary-action hover, and the orange band.

### Secondary
- **Printed Teal**: The second accent for code separators, active account details, and the teal band.

### Neutral
- **Matte Ink**: The dark page shell, primary button, and primary text on paper.
- **Warm Paper**: The account and checkout panels, and button text against ink.
- **Muted Paper**: Supporting copy on the dark shell.
- **Paper Rule**: Dividers and tab underlines on paper.
- **Field White**: The slightly brighter inset used behind editable fields.
- **Record Ink**: The dark product-record surface within the shell.
- **Deep Amber / Deep Teal**: Higher-contrast code accents on warm-paper panels.

**The Ink-and-Paper Rule.** Keep ink and warm paper as the principal surfaces; use amber and teal as compact record marks, reserving their deeper variants for paper-panel text.

## Typography

**Display Font:** IBM Plex Sans (self-hosted; with Segoe UI, Arial, sans-serif fallback)  
**Body Font:** Segoe UI (with Arial, sans-serif fallback)  
**Label/Mono Font:** No separate mono face; prices use tabular numerals in the sans-serif stack.

**Character:** A compact, sturdy IBM Plex Sans headline gives the numbered catalogue a distinct voice; familiar Segoe UI keeps functional copy and controls quiet. Tight headline tracking and widely tracked uppercase labels distinguish task hierarchy from record metadata.

### Hierarchy
- **Headline** (IBM Plex Sans 700, responsive 42–58px, 1.04 line-height, -.045em tracking): The main task heading.
- **Title** (IBM Plex Sans 700, 26px, 1.15 line-height): Panel headings.
- **Body** (400, 16px, 1.5 line-height): Standard explanatory and interface copy.
- **Label** (750, 11px, .17em tracking, uppercase): Compact record codes and metadata; local control labels use the same sans-serif family at a more restrained size.

**The Local Headline Rule.** Bundle IBM Plex Sans for headings; keep Segoe UI for body copy and conventional controls.

**The Tabular Amount Rule.** Use tabular numerals for displayed prices and amounts; the implementation does not use a separate monospaced typeface.

## Layout

Centered page shells use bounded widths rather than edge-to-edge content: the account and checkout layouts cap at 1120px, the store at 1000px, and the header content at 1200px. The account and checkout begin as two columns, then stack at 760px; the product record also stacks to one column at that breakpoint. Side gutters reduce from 24px to 20px on narrow screens. Repeated form spacing is compact (16px), with 30px panel insets on wide layouts and 24px on narrow ones.

## Elevation & Depth

The system is flat: no box shadows are used. Depth comes from the contrast between the ink shell, warm-paper panels, darker product record, and thin borders or dividers. One brief, staggered band-print motion uses a 560ms exponential ease-out and collapses under reduced-motion preferences.

## Shapes

Corners stay square (0px) across panels, fields, and buttons. Thin strokes define fields, rules, and the product record instead of rounded containers or decorative outlines. Controls retain familiar browser semantics and keyboard focus visibility.

## Components

### Buttons
- **Shape:** Square corners (0px), with a 1px ink border.
- **Primary:** Ink fill and warm-paper text; full-width by default, at least 48px tall, with 12px 18px padding and tracked uppercase text.
- **Hover / Focus:** Hover switches to amber with ink text (140ms color transition). Keyboard focus uses a 3px amber outline with a 3px offset. Disabled actions use reduced opacity and a wait cursor.

### Cards / Containers
- **Corner Style:** Square (0px).
- **Background:** Warm paper for account and checkout panels; darker record ink for the product record.
- **Shadow Strategy:** Flat; separation comes from color and borders.
- **Border:** Product records use a thin muted border; panels are unrounded.
- **Internal Padding:** 30px on wide panels, 24px on narrow panels; product records use 22px.

### Inputs / Fields
- **Style:** Off-white fill, 1px muted stroke, square corners, and 44px field height; the amount control is 46px high and uses tabular figures.
- **Focus:** A 3px amber outline with 3px offset, shared with buttons and links.
- **Error / Disabled:** Disabled inputs shift to a subdued paper fill; notices use bordered, tinted status surfaces.

### Navigation
- **Style:** A compact horizontal header separates the wordmark and local-test status or return link with a bottom rule. Links use the paper/ink palette and conventional underlining on interaction.

### Record Codes
Compact uppercase codes use amber on the dark shell, deeper amber on paper, and a teal slash as the separator. They identify the account, catalog, and payment without adding decorative iconography.

### Record Stripe
The five short color bands are a compact catalogue mark. Their staggered scale-in remains fully visible throughout and is the only authored entrance motion.

## Do's and Don'ts

### Do:
- **Do** use ink and warm paper as the primary surface contrast, with amber and teal for concise record marks.
- **Do** keep amounts aligned with tabular numerals and preserve the square, bordered control language.
- **Do** bundle IBM Plex Sans for headings and keep Segoe UI for body and control copy.
- **Do** retain clear keyboard focus outlines on interactive controls.
- **Do** theme text carets and scrollbars from the palette, and respect reduced-motion preferences.

### Don't:
- **Don't** turn the single test-item catalogue into a dense multi-product storefront.
- **Don't** introduce rounded panel or field corners, shadows, or a separate mono face as if they were established patterns.
- **Don't** replace the bundled heading face with an unbundled system display font.
- **Don't** use the bright accent colors as long-form body-text colors.
