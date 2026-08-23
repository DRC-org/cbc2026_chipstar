#include <sensor_msgs/msg/joy.hpp>

#include <rclcpp/logging.hpp>
#include <rclcpp/node.hpp>
#include <rclcpp/rclcpp.hpp>
#include <rclcpp/subscription.hpp>

#include <cerrno>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <fcntl.h>
#include <memory>
#include <sstream>
#include <string>
#include <sys/types.h>
#include <termios.h>
#include <unistd.h>

namespace {

speed_t to_baud_constant(int baud_rate) {
    switch (baud_rate) {
    case 9600:
        return B9600;
    case 19200:
        return B19200;
    case 38400:
        return B38400;
    case 57600:
        return B57600;
    case 115200:
        return B115200;
    case 230400:
        return B230400;
    default:
        return B115200;
    }
}

double axis_at(const sensor_msgs::msg::Joy &msg, std::size_t index) {
    return index < msg.axes.size() ? msg.axes[index] : 0.0;
}

int button_at(const sensor_msgs::msg::Joy &msg, std::size_t index) {
    return index < msg.buttons.size() ? msg.buttons[index] : 0;
}

int percent(double value) {
    return static_cast<int>(std::lround(value * 100.0));
}

std::string format_controller_input(const sensor_msgs::msg::Joy &msg) {
    char line[160];
    std::snprintf(
        line,
        sizeof(line),
        "LX%+04d LY%+04d RX%+04d RY%+04d L2%+04d R2%+04d B%d%d%d%d%d%d%d%d%d%d%d%d%d%d%d%d%d",
        percent(axis_at(msg, 0)),
        percent(axis_at(msg, 1)),
        percent(axis_at(msg, 2)),
        percent(axis_at(msg, 3)),
        percent(axis_at(msg, 4)),
        percent(axis_at(msg, 5)),
        button_at(msg, 0),
        button_at(msg, 1),
        button_at(msg, 2),
        button_at(msg, 3),
        button_at(msg, 4),
        button_at(msg, 5),
        button_at(msg, 6),
        button_at(msg, 7),
        button_at(msg, 8),
        button_at(msg, 9),
        button_at(msg, 10),
        button_at(msg, 11),
        button_at(msg, 12),
        button_at(msg, 13),
        button_at(msg, 14),
        button_at(msg, 15),
        button_at(msg, 16));

    return std::string(line);
}

} // namespace

class CctlUsbCdcBridge : public rclcpp::Node {
  public:
    CctlUsbCdcBridge()
        : Node("cctl_usb_cdc_bridge") {
        serial_device_ = declare_parameter<std::string>("serial_device", "/dev/ttyACM0");
        baud_rate_ = declare_parameter<int>("baud_rate", 115200);
        const auto topic_name = declare_parameter<std::string>("controller_input_topic", "controller_input");

        controller_input_sub_ = create_subscription<sensor_msgs::msg::Joy>(
            topic_name,
            10,
            [this](const sensor_msgs::msg::Joy::SharedPtr msg) {
                send_controller_input(*msg);
            });

        open_serial();
    }

    ~CctlUsbCdcBridge() override {
        if (serial_fd_ >= 0) {
            close(serial_fd_);
        }
    }

  private:
    rclcpp::Subscription<sensor_msgs::msg::Joy>::SharedPtr controller_input_sub_;
    std::string serial_device_;
    int baud_rate_;
    int serial_fd_ = -1;

    void open_serial() {
        if (serial_fd_ >= 0) {
            return;
        }

        serial_fd_ = open(serial_device_.c_str(), O_RDWR | O_NOCTTY | O_NONBLOCK);
        if (serial_fd_ < 0) {
            RCLCPP_WARN_THROTTLE(
                get_logger(),
                *get_clock(),
                2000,
                "failed to open %s: %s",
                serial_device_.c_str(),
                std::strerror(errno));
            return;
        }

        termios tty {};
        if (tcgetattr(serial_fd_, &tty) != 0) {
            RCLCPP_WARN(get_logger(), "tcgetattr(%s) failed: %s", serial_device_.c_str(), std::strerror(errno));
            close(serial_fd_);
            serial_fd_ = -1;
            return;
        }

        cfmakeraw(&tty);
        const speed_t baud = to_baud_constant(baud_rate_);
        cfsetispeed(&tty, baud);
        cfsetospeed(&tty, baud);
        tty.c_cflag |= static_cast<tcflag_t>(CLOCAL | CREAD);
        tty.c_cflag &= static_cast<tcflag_t>(~CRTSCTS);
        tty.c_cc[VMIN] = 0;
        tty.c_cc[VTIME] = 1;

        if (tcsetattr(serial_fd_, TCSANOW, &tty) != 0) {
            RCLCPP_WARN(get_logger(), "tcsetattr(%s) failed: %s", serial_device_.c_str(), std::strerror(errno));
            close(serial_fd_);
            serial_fd_ = -1;
            return;
        }

        RCLCPP_INFO(get_logger(), "opened %s for controller_input", serial_device_.c_str());
    }

    void send_controller_input(const sensor_msgs::msg::Joy &msg) {
        open_serial();
        if (serial_fd_ < 0) {
            return;
        }

        std::string payload = format_controller_input(msg);
        payload.push_back('\n');

        const ssize_t written = write(serial_fd_, payload.data(), payload.size());
        if (written < 0) {
            RCLCPP_WARN_THROTTLE(
                get_logger(),
                *get_clock(),
                2000,
                "write(%s) failed: %s",
                serial_device_.c_str(),
                std::strerror(errno));
            close(serial_fd_);
            serial_fd_ = -1;
            return;
        }

        RCLCPP_DEBUG(get_logger(), "sent %zd bytes to %s", written, serial_device_.c_str());
    }
};

int main(int argc, char **argv) {
    rclcpp::init(argc, argv);
    rclcpp::spin(std::make_shared<CctlUsbCdcBridge>());
    rclcpp::shutdown();
    return 0;
}
