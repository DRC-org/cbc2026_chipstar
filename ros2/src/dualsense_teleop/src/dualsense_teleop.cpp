#include <sensor_msgs/msg/joy.hpp>

#include <functional>
#include <memory>
#include <rclcpp/executors.hpp>
#include <rclcpp/logging.hpp>
#include <rclcpp/node.hpp>
#include <rclcpp/publisher.hpp>
#include <rclcpp/subscription.hpp>

#include <string>

class DualsenseTeleop : public rclcpp::Node {
  public:
    DualsenseTeleop()
        : Node("dualsense_teleop") {
        controller_input_pub_ = create_publisher<sensor_msgs::msg::Joy>("controller_input", 10);
        joy_sub_ = create_subscription<sensor_msgs::msg::Joy>("joy", 10, std::bind(&DualsenseTeleop::on_joy, this, std::placeholders::_1));
    }

  private:
    rclcpp::Publisher<sensor_msgs::msg::Joy>::SharedPtr controller_input_pub_;
    rclcpp::Subscription<sensor_msgs::msg::Joy>::SharedPtr joy_sub_;

    void on_joy(const sensor_msgs::msg::Joy::SharedPtr msg) {
        controller_input_pub_->publish(*msg);

        if (msg->axes.size() < 6 || msg->buttons.size() < 17) {
            RCLCPP_WARN_THROTTLE(
                get_logger(),
                *get_clock(),
                2000,
                "controller_input: axes=%zu buttons=%zu",
                msg->axes.size(),
                msg->buttons.size());
            return;
        }

        RCLCPP_INFO_THROTTLE(
            get_logger(),
            *get_clock(),
            1000,
            "controller_input: axes=%zu buttons=%zu left=(%.2f, %.2f) right=(%.2f, %.2f) l2=%.2f r2=%.2f",
            msg->axes.size(),
            msg->buttons.size(),
            msg->axes[0],
            msg->axes[1],
            msg->axes[2],
            msg->axes[3],
            msg->axes[4],
            msg->axes[5]);
    };
};

int main(int argc, char **argv) {
    rclcpp::init(argc, argv);
    rclcpp::spin(std::make_shared<DualsenseTeleop>());
    rclcpp::shutdown();
    return 0;
}
