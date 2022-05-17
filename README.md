# Mock Payment engine

Engine will maintain client accounts while reading from input various transactions and update client properties accordingly.

Client definition:

- id: u16 representing a unique client to the system
- available: Total funds available for client to use. Equal to total - held.
- held: Total funds that are held for disputes. Equal to total - available.
- total: Total of funds from available and held. Equal to available + held
- locked: Whether account is locked, which happens if a successful charge back occurs for client

Clients are unique, and transactions stop applying to them when the account becomes locked.
The fields available, held, and total are all decimal's with a precision of 4.

## Input

Reads in a specified CSV file from first os arg

Each line is a transaction for the client that specifies a transaction type and transaction tx.
A transaction tx is a globally unique u32 id.

- type: one of 5 string enum values
  - deposit: Credit to client account available funds
  - withdrawal: Debit to client available funds, if available funds >= amount specified
  - dispute: Debits clients available funds and credits it to clients held funds for the amount of tx specified's transaction
    - If tx does not exist, or is for a different client, assume error on part of partner
    - Skipped if clients available funds less than the specified amount of the transaction, similar to withdrawal
  - resolve: Debits clients held funds and credits it to available funds for the amount of the tx specified's transaction
    - If tx's client that deposited transaction does not match line being processed's client, record is skipped and error assumed on part of partner
    - If client doesn't have an existing dispute for that transaction, resolve is skipped and error assumed on part of partner
  - chargeback: Debits clients held funds and locks the account
    - If tx's client that deposited transaction does not match record being processed's client, record is skipped and error assumed on part of partner
    - If client doesn't have an existing dispute for that transaction, charge back is skipped and error assumed on part of partner


Expected format:

For type deposit and withdrawal:

```
type,   client,     tx,     amount
deposit,    1,      1,         1.0
```

- tx in this case should not exist previously

For type dispute, resolve, chargeback:

```
type,   client,     tx
deposit,    1,      1
```

- tx in this case signifies a previous deposit transaction that is in dispute

## Output

Once all lines have been processed without error, the executeable writes accounts to stdout in csv format

Example output:

```
client, available,  held,   total,  locked
1,2.3245,0.0000,2.3245,false
2,1.0000,2.0000,3.0000,false
3,2.2500,0.0000,2.2500,true
```

- ordering by client id not gauranteed and not required
- balances are to a precision of 4

## Assumptions

- I assume that disputes can only occur on deposits due to the way the requirements are written
- A dispute can't occur if the available have equal to or more than the disputed amount of the transaction
    - Would cause a negative balance on the account if charge back transaction on dispute occurs
    - This could be a very wrong assumption though
- I assume partner error if a tx id appears twice for deposit or withdrawal type records

## Testing

I tested using files in the inputs/ folder.

## Improvements (that I know of)

- Possibly avoiding rescale until display, rounding to precision 4 before writing to stdout
- Knowing idiomatic Rust better
- Refactoring into clean code with more specific function responsibilities
    - Like moving client and transaction and their implementations to separate files
- Using actual tests and running cargo test

