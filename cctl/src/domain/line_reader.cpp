#include "domain/line_reader.hpp"

namespace domain {

bool LineReader::push(char ch) {
    if (ch != '\n') {
        if (!overflow_) {
            if (length_ < CAPACITY) {
                buffer_[length_++] = ch;
            } else {
                overflow_ = true;
            }
        }
        return false;
    }

    const bool complete = !overflow_;
    if (complete) {
        completed_length_ = length_;
        // CRLF で送られてきた場合の CR を落とす。
        if (completed_length_ > 0 && buffer_[completed_length_ - 1] == '\r') {
            --completed_length_;
        }
    }

    length_ = 0;
    overflow_ = false;
    return complete;
}

}  // namespace domain
