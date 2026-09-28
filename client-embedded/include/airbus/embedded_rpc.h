#pragma once

/**
 * Schema-free Airbus JSON-RPC client for embedded (TCP :9097).
 * Typed payloads from generated airbus/payloads.h; daemon validates schemas.
 */

#include <Arduino.h>
#include <ArduinoJson.h>

#include <airbus/payloads.h>

namespace airbus {

class EmbeddedRpcClient {
 public:
  EmbeddedRpcClient() = default;
  EmbeddedRpcClient(const char *host, uint16_t port,
                    uint32_t timeout_ms = 8000);

  void set_endpoint(const char *host, uint16_t port);
  void set_timeout_ms(uint32_t timeout_ms);

  const String &host() const { return host_; }
  uint16_t port() const { return port_; }

  /** Raw JSON-RPC call. params may be nullptr (omit params). */
  bool call(const char *method, JsonDocument *params, JsonDocument &result,
            String &err);

  bool ping(String &err);

  bool add(const AddParams &params, AddResult &result, String &err);

  bool create_queue(const CreateQueueParams &params, CreateQueueResult &result,
                    String &err);

  bool delete_queue(const DeleteQueueParams &params, DeleteQueueResult &result,
                    String &err);

  bool post_event(const PostEventParams &params, PostEventResult &result,
                  String &err);

  bool list_queues(ListQueuesResult &result, String &err);

  bool peek_events(const PeekEventsParams &params, PeekEventsResult &result,
                   String &err);

  bool attach_listener(const AttachListenerParams &params,
                       AttachListenerResult &result, String &err);

  bool detach_listener(const DetachListenerParams &params,
                       DetachListenerResult &result, String &err);

  bool list_listeners(const ListListenersParams &params,
                      ListListenersResult &result, String &err);

  bool queue_ready(const QueueReadyParams &params, QueueReadyResult &result,
                   String &err);

  /**
   * Convenience: list_listeners for queue and report whether listener_id is
   * present and active. On RPC failure, found_active is left unchanged.
   */
  bool listener_is_active(const char *queue, const char *listener_id,
                          bool &found_active, String &err);

 private:
  template <typename Params, typename Result>
  bool invoke_object(const char *method, const Params &params, Result &result,
                     String &err);

  template <typename Result>
  bool invoke_no_params(const char *method, Result &result, String &err);

  String host_;
  uint16_t port_ = 9097;
  uint32_t timeout_ms_ = 8000;
  int next_id_ = 1;
};

}  // namespace airbus
