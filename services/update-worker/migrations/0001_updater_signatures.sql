-- 现有统一更新数据库的增量迁移；历史版本与其它应用记录保留。
ALTER TABLE releases ADD COLUMN signature TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS releases_identity ON releases(app_id, platform, version);
CREATE INDEX IF NOT EXISTS releases_latest ON releases(app_id, platform, is_latest);
