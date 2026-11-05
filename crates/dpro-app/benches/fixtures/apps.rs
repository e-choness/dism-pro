//! The U11 table (TD-0009 §7): 100k installed programs. Shared by the criterion bench
//! (`table.rs`) and the instruction-count bench (`ir.rs`).

use dpro_app::table::{Cell, ColumnId, RowKey, RowSource};
use dpro_core::units::{Bytes, UnixMillis};

/// S-01's list size.
pub(crate) const ROWS: u32 = 100_000;
pub(crate) const NAME: ColumnId = ColumnId(0);
pub(crate) const PUBLISHER: ColumnId = ColumnId(1);
pub(crate) const VERSION: ColumnId = ColumnId(2);
pub(crate) const SIZE: ColumnId = ColumnId(3);
pub(crate) const INSTALLED: ColumnId = ColumnId(4);
/// The columns the filter searches.
pub(crate) const SEARCHED: [ColumnId; 3] = [NAME, PUBLISHER, VERSION];

/// Installed programs, with the Uninstaller's columns (artboard 08). Long shared prefixes
/// ("Contoso Updater Pro …") and embedded numbers are the expensive case for natural sorting.
pub(crate) struct Apps {
    name: Vec<String>,
    publisher: Vec<String>,
    version: Vec<String>,
}

impl Apps {
    pub(crate) fn new(n: u32) -> Self {
        const VENDORS: [&str; 8] = [
            "Contoso",
            "Fabrikam",
            "Northwind",
            "Tailspin",
            "Litware",
            "Adatum",
            "Woodgrove",
            "Proseware",
        ];
        const PRODUCTS: [&str; 8] = [
            "Updater Pro",
            "Toolbox",
            "Media Suite",
            "Backup Agent",
            "PDF Studio",
            "Driver Kit",
            "VPN Client",
            "Office Helper",
        ];
        let n = n as usize;
        let mut a = Self {
            name: Vec::with_capacity(n),
            publisher: Vec::with_capacity(n),
            version: Vec::with_capacity(n),
        };
        for i in 0..n {
            // Interleaved, so neither the name nor the size order follows the row order.
            let v = VENDORS[(i * 7) % 8];
            a.name.push(format!(
                "{v} {} {}",
                PRODUCTS[(i * 3) % 8],
                (i * 7919) % 100_000
            ));
            a.publisher.push(format!("{v} Corporation"));
            a.version
                .push(format!("{}.{}.{}", i % 13, i % 100, i % 997));
        }
        a
    }
}

impl RowSource for Apps {
    fn len(&self) -> u32 {
        u32::try_from(self.name.len()).unwrap_or(u32::MAX)
    }
    fn key(&self, row: u32) -> RowKey {
        RowKey(u64::from(row))
    }
    fn cell(&self, row: u32, col: ColumnId) -> Cell<'_> {
        let r = row as usize;
        match col {
            NAME => Cell::Text(&self.name[r]),
            PUBLISHER => Cell::Text(&self.publisher[r]),
            VERSION => Cell::Mono(&self.version[r]),
            SIZE => Cell::Size(Bytes(
                u64::from(row.wrapping_mul(2_654_435_761)) % 4_000_000_000,
            )),
            INSTALLED => Cell::Time(UnixMillis(1_600_000_000_000 + u64::from(row) * 60_000)),
            _ => Cell::Empty,
        }
    }
}
