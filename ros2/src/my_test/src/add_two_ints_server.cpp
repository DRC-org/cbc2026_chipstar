#include <memory>
#include <rclcpp/executors.hpp>
#include <rclcpp/node.hpp>
#include <rclcpp/node_interfaces/node_base_interface.hpp>
#include <rclcpp/service.hpp>
#include <rclcpp/utilities.hpp>

#include "example_interfaces/srv/add_two_ints.hpp"
#include "rclcpp/rclcpp.hpp"

using example_interfaces::srv::AddTwoInts;

class AddTwoIntsServer : public rclcpp::Node {
  public:
    AddTwoIntsServer() : Node("add_two_ints_server") {
        service_ = create_service<AddTwoInts>(
            "add_two_ints",
            [this](const std::shared_ptr<AddTwoInts::Request> request,
                   std::shared_ptr<AddTwoInts::Response> response) {
                response->sum = request->a + request->b;
                RCLCPP_INFO(
                    get_logger(),
                    "request: a=%ld b=%ld -> sum=%ld",
                    request->a,
                    request->b,
                    response->sum);
            });
    }

  private:
    rclcpp::Service<AddTwoInts>::SharedPtr service_;
};

int main(int argc, char **argv) {
    rclcpp::init(argc, argv);
    rclcpp::spin(std::make_shared<AddTwoIntsServer>());
    rclcpp::shutdown();
    return 0;
}
