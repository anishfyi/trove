// Ordered publishing to the event log.
export async function publishEvent(producer, topic, event) {
  return producer.send({
    topic,
    messages: [event],
    key: event.aggregateId,
  });
}
