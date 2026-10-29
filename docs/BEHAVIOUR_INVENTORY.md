# Behaviour inventory

Measured, not estimated. The behaviour registry was enumerated at runtime
(`tools` + shim dispatch hook) and every goal and plan dispatch was counted with
the clock unpinned and the sampler off, across three play sessions: solo cat,
cat+dog, and three pets abandoned in Arabia without food or water.

Raw data: `evidence/trace/names.txt` (id -> name) and `evidence/trace/dispatch.txt`
(`kind id count`).

## Population

```
goals registered: 114
plans registered: 112
```

The 241 "Goal*" exports in the binary are mostly support classes - `GoalToken`,
`GoalManager`, `GoalDescriptor`, `GoalKey` - not behaviour types.

## Coverage from three sessions

```
goals: 38/114 seen (33%)   703 dispatches
plans: 36/112 seen (32%)   41674 dispatches
```

Plans outnumber goals ~59:1. A goal is chosen rarely; its plan runs every tick.
That ratio is the architecture in one number.

## Goals that ran

| goal | dispatches |
|---|---|
| pause | 116 |
| wrestlePet | 92 |
| idleWithPet | 69 |
| nipAtPet | 41 |
| greetPet | 35 |
| groomOther | 35 |
| eat | 27 |
| followSprite | 25 |
| lookAroundAtToys | 24 |
| bondWithBuddy | 23 |
| givingGiftsTogether | 22 |
| catNap | 19 |
| affectionToCursor | 17 |
| comfortPet | 17 |
| groom | 13 |
| runawayFromSprite | 13 |
| adultAndChildTogether | 12 |
| naughty | 12 |
| greetUser | 11 |
| investigateSprite | 11 |
| nudged | 10 |
| tossSpriteWithPet | 8 |
| sleepTogether | 7 |
| gravityGame | 6 |
| insanity | 6 |
| sniffSprite | 5 |
| emotional | 4 |
| idle | 4 |
| sharpenClawsOnSprite | 4 |
| snubSprite | 3 |
| beOnLedge | 2 |
| hoardSprites | 2 |
| sprayed | 2 |
| watchClouds | 2 |
| burySprite | 1 |
| playFetch | 1 |
| rollOnGround | 1 |
| stalkAttackSprite | 1 |

## Plans that ran (top 25)

| plan | dispatches |
|---|---|
| getFaceToFace | 10245 |
| metascript | 4653 |
| maltoscript | 3914 |
| eatDrinkFromBowl | 3063 |
| wrestlePet | 2607 |
| followSprite | 2267 |
| ambushPet | 1670 |
| sleepTogether | 1618 |
| naughtyMakeAMess | 1496 |
| naughtyMetascript | 1492 |
| affectionToCursor | 1389 |
| investigateSprite | 1311 |
| catNap | 1192 |
| runAwayFromSprite | 942 |
| watchClouds | 673 |
| sniffSprite | 449 |
| lookAroundAtToys | 422 |
| swatSprite | 322 |
| insanity | 275 |
| pickUpSprite | 253 |
| sharpenClaws | 251 |
| catMoment | 200 |
| nudged | 200 |
| snubSprite | 170 |
| tossSpriteWithPet | 140 |

## What did *not* run

Worth recording, because absence is data. Leaving three pets without food or
water for several minutes did **not** fire `actNeglected`, `lonely`,
`markTerritory`, `stealToy`, `greetNewSpecies` or `sinkDrink`. Either those
timers are longer than a few minutes, or their preconditions were not met (for
example `greetNewSpecies` may need a genuinely new breed, not another cat).

An earlier pass claimed these had fired - that was a bug in the analysis: it
listed registry *names* rather than dispatched ids. The corrected figure is
above.

## Caveats

- One player, one house, three sessions. Different species, kittens, illness,
  breeding and multiple rooms would select different goals.
- The counters are cumulative per process; a long session weights toward
  whichever plan is sticky.
- `insanity` fired 6 times and `naughty*` plans run heavily when the pet is
  under-stimulated. These are emergent from goal arbitration, not scripted.
