use core::borrow::Borrow;
use core::hash::Hash;
use core::hash::Hasher;
use csv::StringRecord;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self};
use std::{env, process};

type TxId = u32;
type ClientId = u16;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
enum TransactionType {
    Deposit,
    Withdrawal,
    Dispute,
    Resolve,
    Chargeback,
}

impl FromStr for TransactionType {
    type Err = ();

    fn from_str(s: &str) -> Result<TransactionType, Self::Err> {
        match s {
            "deposit" => Ok(TransactionType::Deposit),
            "withdrawal" => Ok(TransactionType::Withdrawal),
            "dispute" => Ok(TransactionType::Dispute),
            "resolve" => Ok(TransactionType::Resolve),
            "chargeback" => Ok(TransactionType::Chargeback),
            _ => Err(()),
        }
    }
}

#[derive(Eq, Serialize, Deserialize, Debug, Clone)]
struct Transaction {
    id: TxId,
    transaction_type: TransactionType,
    client_id: ClientId,
    amount: Decimal,
}

impl PartialEq for Transaction {
    fn eq(&self, other: &Transaction) -> bool {
        self.id == other.id
    }
}

impl Hash for Transaction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Borrow<TxId> for Transaction {
    fn borrow(&self) -> &TxId {
        &self.id
    }
}

#[derive(Eq, Clone, Debug, Serialize)]
struct Client {
    #[serde(rename(serialize = "client"))]
    id: ClientId,
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
    #[serde(skip_serializing)]
    disputes: HashSet<TxId>,
}

impl PartialEq for Client {
    fn eq(&self, other: &Client) -> bool {
        self.id == other.id
    }
}

impl Hash for Client {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl Borrow<ClientId> for Client {
    fn borrow(&self) -> &ClientId {
        &self.id
    }
}

impl Client {
    fn new(id: ClientId) -> Client {
        Client {
            id: id,
            available: Decimal::from_str("0.0000").unwrap(),
            held: Decimal::from_str("0.0000").unwrap(),
            locked: false,
            total: Decimal::from_str("0.0000").unwrap(),
            disputes: HashSet::<TxId>::new(),
        }
    }
    fn handle_transaction(
        &mut self,
        transaction_type: &TransactionType,
        transaction: &Transaction,
    ) {
        // Client is locked, no further handling should occur (far as I understand)
        if self.locked {
            return;
        }
        use TransactionType::*;
        match transaction_type {
            Deposit => self.deposit(transaction.amount),
            Withdrawal => self.withdrawal(transaction.amount),
            Dispute => self.dispute(
                transaction.id,
                &transaction.transaction_type,
                transaction.amount,
            ),
            Resolve => self.resolve(transaction.id, transaction.amount),
            Chargeback => self.chargeback(transaction.id, transaction.amount),
        }
        self.calculate_total();
    }

    fn deposit(&mut self, amount: Decimal) {
        self.available = self.available + amount;
    }

    fn calculate_total(&mut self) {
        self.total = self.available + self.held;
    }

    fn withdrawal(&mut self, amount: Decimal) {
        if self.available >= amount {
            self.available = self.available - amount;
        }
    }

    fn dispute(&mut self, tx_id: TxId, transaction_type: &TransactionType, amount: Decimal) {
        if transaction_type == &TransactionType::Deposit {
            self.disputes.insert(tx_id);
            self.available -= amount;
            self.held += amount;
        }
    }

    fn resolve(&mut self, tx_id: TxId, amount: Decimal) {
        if self.disputes.contains(&tx_id) {
            self.disputes.remove(&tx_id);
            self.available += amount;
            self.held -= amount;
        }
    }

    fn chargeback(&mut self, tx_id: TxId, amount: Decimal) {
        if self.disputes.contains(&tx_id) {
            self.disputes.remove(&tx_id);
            self.held -= amount;
            self.locked = true;
        }
    }
}

struct ToyProgram {
    clients: HashSet<Client>,
    transactions: HashSet<Transaction>,
}

impl ToyProgram {
    fn new() -> ToyProgram {
        let clients = HashSet::<Client>::new();
        let transactions = HashSet::<Transaction>::new();
        ToyProgram {
            clients,
            transactions,
        }
    }

    pub fn process(&mut self) -> Result<(), Box<dyn Error>> {
        let file_path = self.get_from_env()?;
        let file = File::open(file_path)?;
        let mut reader = csv::ReaderBuilder::new().flexible(true).from_reader(file);

        for result in reader.records().skip(1) {
            use TransactionType::*;
            let record = result.unwrap_or_else(|err| {
                panic!("Could not parse csv result to StringResult: {}", err)
            });
            let (transaction_type, transaction) = self.transaction_from_record(record)?;

            match (&transaction_type, &transaction) {
                (Deposit | Withdrawal, None) => {
                    panic!("Deposits and withdrawals require a transaction")
                }
                // No matching transaction, assume partner error
                (Dispute | Resolve | Chargeback, None) => (),
                (Deposit | Withdrawal, Some(t)) => {
                    let unique = self.ensure_globally_unique_transaction(transaction.clone())?;
                    // If no result assume partner error
                    if unique {
                        self.transactions.insert(transaction.clone().unwrap());

                        let mut client = match self.clients.get(&t.client_id) {
                            Some(c) => {
                                let client = c.clone();
                                self.clients.remove(&client);
                                client
                            }
                            None => Client::new(t.client_id),
                        };
                        client.handle_transaction(&transaction_type, &t);
                        self.clients.insert(client);
                    }
                }
                (Dispute | Resolve | Chargeback, Some(t)) => {
                    match self.clients.get(&t.client_id) {
                        Some(c) => {
                            let mut client = c.clone();
                            if client.id == t.client_id {
                                client.handle_transaction(&transaction_type, &t);
                                self.clients.remove(&client.id);
                                self.clients.insert(client);
                            }
                        }
                        None => (),
                    };
                }
            }
        }
        self.display_clients()?;
        Ok(())
    }

    pub fn display_clients(&self) -> Result<(), Box<dyn Error>> {
        let mut writer = csv::Writer::from_writer(io::stdout());
        for client in &self.clients {
            writer.serialize(client)?;
        }
        Ok(())
    }

    fn ensure_globally_unique_transaction(
        &self,
        transaction: Option<Transaction>,
    ) -> Result<bool, Box<dyn Error>> {
        match transaction {
            None => Err(From::from("Transaction doesn't exist")),
            Some(t) => match self.transactions.get(&t.id) {
                None => Ok(true),
                _ => Ok(false),
            },
        }
    }

    fn transaction_from_record(
        &self,
        record: StringRecord,
    ) -> Result<(TransactionType, Option<Transaction>), Box<dyn Error>> {
        use TransactionType::*;
        let transaction_type = record[0]
            .parse::<TransactionType>()
            .unwrap_or_else(|err| panic!("{:?}", err));
        let client_id = record[1]
            .trim()
            .parse::<ClientId>()
            .unwrap_or_else(|err| panic!("Failed to set client_id from {} {}", &record[1], err));
        let tx = record[2]
            .trim()
            .parse::<TxId>()
            .unwrap_or_else(|err| panic!("Failed to set tx from {} {}", &record[2], err));
        match transaction_type {
            Deposit | Withdrawal => {
                let mut amount = Decimal::from_str(&record[3].trim()).unwrap_or_else(|err| {
                    panic!("Failed to set amount from {} {}", &record[3], err)
                });
                amount.rescale(4);
                let transaction = Transaction {
                    id: tx,
                    transaction_type: transaction_type.clone(),
                    client_id: client_id,
                    amount: amount,
                };

                return Ok((transaction_type, Some(transaction)));
            }
            Dispute | Resolve | Chargeback => {
                match self.transactions.get(&tx) {
                    Some(t) => {
                        // Client must own transaction, else record is in error
                        if &t.client_id == &client_id {
                            Ok((transaction_type, Some(t.clone())))
                        } else {
                            // Matching tx id is not relative to client
                            Ok((transaction_type, None))
                        }
                    }
                    None => return Ok((transaction_type, None)),
                }
            }
        }
    }

    fn get_from_env(&self) -> Result<OsString, Box<dyn Error>> {
        match env::args_os().nth(1) {
            None => Err(From::from(
                "Expected 1 argument for transaction csv, but got none",
            )),
            Some(file_path) => Ok(file_path),
        }
    }
}

fn main() {
    let mut service = ToyProgram::new();
    if let Err(err) = service.process() {
        println!("{}", err);
        process::exit(1);
    }
    process::exit(0);
}
