use crate::table::{Table, Value};
use crate::Result;

pub trait Backend {
    fn insert<T: Table>(&mut self, row: T) -> Result<()> {
        self.__insert(T::NAME, row.to_values())
    }

    fn insert_many<T, I>(&mut self, rows: I) -> Result<()>
    where
        T: Table,
        I: IntoIterator<Item = T>,
    {
        self.__insert_many(
            T::NAME,
            rows.into_iter().map(|row| row.to_values()).collect(),
        )
    }

    fn upsert<T: Table>(&mut self, row: T) -> Result<bool> {
        self.__upsert(T::NAME, row.to_values())
    }

    fn upsert_many<T, I>(&mut self, rows: I) -> Result<u64>
    where
        T: Table,
        I: IntoIterator<Item = T>,
    {
        self.__upsert_many(
            T::NAME,
            rows.into_iter().map(|row| row.to_values()).collect(),
        )
    }

    fn get<T: Table>(&self, key: T::Key) -> Result<Option<T>> {
        self.__get(T::NAME, T::key_to_value(&key))?
            .map(T::from_values)
            .transpose()
    }

    fn contains<T: Table>(&self, key: &T::Key) -> Result<bool> {
        self.__contains(T::NAME, T::key_to_value(key))
    }

    fn update<T: Table>(&mut self, row: T) -> Result<bool> {
        self.__update(T::NAME, row.to_values())
    }

    fn patch<T: Table>(&mut self, key: T::Key, patch: T::Patch) -> Result<bool> {
        let Some(mut row) = self.get::<T>(key)? else {
            return Ok(false);
        };
        row.apply_patch(&patch);
        self.update(row)
    }

    fn delete<T: Table>(&mut self, key: T::Key) -> Result<bool> {
        self.__delete(T::NAME, T::key_to_value(&key))
    }

    fn count<T: Table>(&self) -> Result<u64> {
        self.__count(T::NAME)
    }

    fn scan<T: Table>(&self) -> Result<Vec<T>> {
        self.__scan(T::NAME)?
            .into_iter()
            .map(T::from_values)
            .collect()
    }

    fn begin(&mut self) -> Result<()>;

    fn commit(&mut self) -> Result<()>;

    fn rollback(&mut self) -> Result<()>;

    fn in_transaction(&self) -> bool;

    fn transaction<T, F>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
        Self: Sized;

    fn close(self) -> Result<()>
    where
        Self: Sized;

    #[doc(hidden)]
    fn __insert(&mut self, table: &'static str, values: Vec<Value>) -> Result<()>;
    #[doc(hidden)]
    fn __insert_many(&mut self, table: &'static str, rows: Vec<Vec<Value>>) -> Result<()>;
    #[doc(hidden)]
    fn __upsert(&mut self, table: &'static str, values: Vec<Value>) -> Result<bool>;
    #[doc(hidden)]
    fn __upsert_many(&mut self, table: &'static str, rows: Vec<Vec<Value>>) -> Result<u64>;
    #[doc(hidden)]
    fn __get(&self, table: &'static str, key: Value) -> Result<Option<Vec<Value>>>;
    #[doc(hidden)]
    fn __contains(&self, table: &'static str, key: Value) -> Result<bool>;
    #[doc(hidden)]
    fn __update(&mut self, table: &'static str, values: Vec<Value>) -> Result<bool>;
    #[doc(hidden)]
    fn __delete(&mut self, table: &'static str, key: Value) -> Result<bool>;
    #[doc(hidden)]
    fn __count(&self, table: &'static str) -> Result<u64>;
    #[doc(hidden)]
    fn __scan(&self, table: &'static str) -> Result<Vec<Vec<Value>>>;
}
