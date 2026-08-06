-- Topic subscription corrections: support topic-first fan-out reads.
CREATE INDEX IF NOT EXISTS idx_rch_subscribers_topic_node
    ON rch_subscribers (topic_id, node_id);
