// Bulk row loading for the ingest pipeline.
export function runBulkInsert(client, rows) {
  return client.transaction((tx) => {
    const chunks = chunk(rows, 500);
    for (const c of chunks) tx.exec("insert into events values (?)", c);
  });
}

function chunk(rows, size) {
  const out = [];
  for (let i = 0; i < rows.length; i += size) out.push(rows.slice(i, i + size));
  return out;
}
