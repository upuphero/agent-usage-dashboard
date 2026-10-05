import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { DatabaseSync } from 'node:sqlite';

const migration = readFileSync(new URL('../migrations/001_initial.sql', import.meta.url), 'utf8');
function database() {
  const db = new DatabaseSync(':memory:');
  db.exec('PRAGMA foreign_keys=ON; BEGIN IMMEDIATE');
  db.exec(migration); db.exec('COMMIT');
  return db;
}
function seed(db) {
  db.prepare('INSERT INTO devices VALUES(?)').run('device');
  db.prepare('INSERT INTO source_datasets VALUES(?,?,?,?)').run('dataset','ccusage.claude-code','claude-code','device');
  db.prepare('INSERT INTO report_snapshots VALUES(?,?,?,?,?,?,?,?,?)').run('snapshot','dataset','ccusage.claude-code','claude-code','daily','UTC','"Standard"',1,'{}');
}
const insert = `INSERT INTO report_rows(snapshot_key,row_key,dimension_kind,local_date,model_key,input_accuracy,cache_read_accuracy,cache_write_accuracy,output_accuracy,reasoning_accuracy,total,total_accuracy,cost_accuracy,missing_models_json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)`;
function row(db, value, totalAccuracy = 'exact', key = 'row') {
  db.prepare(insert).run('snapshot',key,'day','2026-10-04','null','unavailable','unavailable','unavailable','unavailable','unavailable',value,totalAccuracy,'unavailable','[]');
}
test('migration creates strict tables with foreign keys, indexes and exact signed 64-bit tokens', () => {
  const db = database(); seed(db); row(db,9223372036854775807n);
  const read = db.prepare('SELECT total,typeof(total) AS storageType,output_reasoning FROM report_rows'); read.setReadBigInts(true);
  assert.deepEqual({...read.get()},{total:9223372036854775807n,storageType:'integer',output_reasoning:null});
  assert.equal(db.prepare('PRAGMA foreign_keys').get().foreign_keys,1);
  assert.ok(db.prepare('SELECT name FROM sqlite_master WHERE name=?').get('rows_day'));
  assert.throws(()=>row(db,1,'exact'),/UNIQUE/);
  assert.throws(()=>row(db,1.5,'exact','fractional'),/REAL|INTEGER/);
  assert.throws(()=>row(db,null,'exact','unknown-as-known'),/CHECK/);
  db.close();
});
test('snapshot row replacement rolls back on SQL failure and committed deletes cascade', () => {
  const db=database();seed(db);row(db,10);
  db.exec('BEGIN IMMEDIATE;DELETE FROM report_rows');
  assert.throws(()=>row(db,-1),/CHECK/);db.exec('ROLLBACK');
  assert.equal(db.prepare('SELECT total FROM report_rows').get().total,10);
  db.prepare('DELETE FROM report_snapshots WHERE snapshot_key=?').run('snapshot');
  assert.equal(db.prepare('SELECT count(*) AS n FROM report_rows').get().n,0);db.close();
});
test('DDL migration failure can be rolled back without removing older tables', () => {
  const db=new DatabaseSync(':memory:');db.exec('CREATE TABLE devices(legacy TEXT);BEGIN IMMEDIATE');
  assert.throws(()=>db.exec(migration),/already exists/);db.exec('ROLLBACK');
  assert.equal(db.prepare('SELECT count(*) AS n FROM sqlite_master WHERE name=?').get('schema_migrations').n,0);
  assert.ok(db.prepare('SELECT name FROM sqlite_master WHERE name=?').get('devices'));db.close();
});
