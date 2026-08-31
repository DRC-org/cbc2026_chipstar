#include "domain/can_frame.hpp"

namespace domain {

Axis classifyFeedback(const CanFrame& frame, uint16_t theta_feedback_id,
                      uint16_t z_feedback_id) {
    if (frame.extended) {
        return Axis::R;
    }

    const uint16_t id = static_cast<uint16_t>(frame.id);
    if (id == theta_feedback_id) {
        return Axis::Theta;
    }
    if (id == z_feedback_id) {
        return Axis::Z;
    }
    return Axis::None;
}

}  // namespace domain
