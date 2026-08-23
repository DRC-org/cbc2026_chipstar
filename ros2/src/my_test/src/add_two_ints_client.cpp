#include <chrono>
#include <cstdint>
#include <cstdlib>
#include <memory>
#include <queue>
#include <rclcpp/client.hpp>
#include <rclcpp/create_client.hpp>
#include <rclcpp/executors.hpp>
#include <rclcpp/future_return_code.hpp>
#include <rclcpp/logger.hpp>
#include <rclcpp/logging.hpp>
#include <rclcpp/node.hpp>
#include <rclcpp/utilities.hpp>
#include <string>
#include <unistd.h>

#include "example_interfaces/srv/add_two_ints.hpp"
#include "rclcpp/rclcpp.hpp"

using namespace std::chrono_literals;
using example_interfaces::srv::AddTwoInts;

class AddTwoIntsClient : public rclcpp::Node {
  public:
    AddTwoIntsClient() : Node("add_two_ints_client") {
        client_ = create_client<AddTwoInts>("add_two_ints");
    }

    void send_request(int64_t a, int64_t b) {
        while (!client_->wait_for_service(1s)) {
            if (!rclcpp::ok()) {
                return;
            }
            RCLCPP_INFO(get_logger(), "waiting for service ..");
        }
        auto request = std::make_shared<AddTwoInts::Request>();
        request->a = a;
        request->b = b;

        auto future = client_->async_send_request(request);

        if (rclcpp::spin_until_future_complete(shared_from_this(), future) == rclcpp::FutureReturnCode::SUCCESS) {
            RCLCPP_INFO(get_logger(), "sum: %ld", future.get()->sum);
        } else {
            RCLCPP_ERROR(get_logger(), "failed to call service");
        }
    }

  private:
    rclcpp::Client<AddTwoInts>::SharedPtr client_;
};

int main(int argc, char **argv) {
    rclcpp::init(argc, argv);

    if (argc != 3) {
        RCLCPP_ERROR(rclcpp::get_logger("add_two_ints_client"),
                     "usage: ros2 run my_robot_cpp add_two_ints_client <a> <b>");
        rclcpp::shutdown();
        return 1;
    }

    auto node = std::make_shared<AddTwoIntsClient>();

    const auto a = std::stoll(argv[1]);
    const auto b = std::stoll(argv[2]);

    node->send_request(a, b);

    rclcpp::shutdown();
    return 0;
}
