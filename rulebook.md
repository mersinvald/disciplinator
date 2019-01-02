## Rule Book

- Every hour there should be N minutes of physical activity
- If there was not enough activity, event triggers and `headmaster` enters the debt collection state
- Until debt is collected, state is not changing.
- During the next hour user must return the debt through serving some physically active time __and__ fulfill the N minutes of this next hour, 
otherwise the debt would need to be collected again and state would be triggered again

## States

There are 2 states:

- Normal
- Debt collection

Application of these states is implementation defined in `driver`s, for instance `driver` may lock the user out of the 
OS or certain applications, disrupt their work and make their life miserable until the debt is collected.

## Sleep time

Punishing rest during sleep is too medieval, so the activity data is cross-referenced with sleep time in such way
that sleep time would be considered active time (for simplicity).
One can try sleeping every second hour if they will.

## Wearables monitoring and corner cases

Since all the user monitoring is conducted via wearable devices (Firbit Charge 2 currently), there might be a situation when these devices 
would go offline for various reasons and won't sync with the server. 
In such circumstances it's not advisable to make user's life miserable as there would be no way for them to revert that no matter how much they train.
Possible solutions:
- For Charge 2: cross-reference timed data with heartrate data, so that if there's no heartrate data for the last hour, there would be no debt collection happening,
as most likely device is not connected then. Though upon synchronization user may find them in unfortunate situation of having more debt then regular N minutes. That's their problem. Setup the continuous sync, for fuck's sake!
- Collect the debt as usual, but leave an opportunity to retrieve the amnesty code from the mediator third person, who cares about the user and is willing to make them suffer for the greater good.

## Failsafe mechanisms

As any software this may fail, charging unreasonable physical activity self-service from the user. To avoid that, it's recommended to limit the maximum debt to N*3.