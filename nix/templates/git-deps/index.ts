import _ from "lodash";
import isEven from "is-even";

// Using _.map to create a new array by applying a function to each element
const squaredNumbers = _.map([1, 2, 3], function (num) {
  return num * num;
});

// Using _.filter to create a new array containing only the elements that satisfy a condition
const evenNumbers = _.filter([1, 2, 3, 4, 5], function (num) {
  return isEven(num);
});

console.log(
  `Squared Numbers: [${squaredNumbers}], Even Numbers: [${evenNumbers}]`,
);
