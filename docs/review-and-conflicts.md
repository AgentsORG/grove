# Reviews and conflicts

## Read a change by meaning

<img src="images/review.png" alt="A change described as a rename, a new parameter and three updated call sites, with the source diff underneath and the evidence beside it" width="100%">

`grove show` lists a change as what it did: renamed `parse` to `parseHeader`, changed `retryDelay`, added `clamp`. Because it knows which declarations it wrote and which it only read, you can see its reach without reading every line. `grove diff c_...` gives the text diff when you want it.

Evidence travels with every landing: which checks ran on the combined result, and whether they passed.

## When work really collides

<img src="images/conflict.png" alt="Two edits to the same line of retryDelay: one statement merged on its own, the clashing line saved as a conflict, and an agent proposing a combined line" width="100%">

Grove only stops a landing when two changes set **the same lines** of a declaration differently. Edits to different lines of the same function are merged. Identical edits aren't a conflict.

A conflict is saved, never lost:

```bash
grove conflicts                # open conflicts
grove conflicts x_b6b6f3       # before, on main, and this change: side by side
```

To resolve it, a person or an agent:

```bash
grove sync                     # bring the workspace up to main
# combine both intents by hand
grove new "Combine cap and backoff"
grove land
grove resolve x_b6b6f3         # close the conflict
```

Main never moved while the conflict was open, so nothing anyone else was doing was blocked.
