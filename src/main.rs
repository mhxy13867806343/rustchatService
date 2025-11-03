fn main() {
    let  x: i32 = 5;
    let  y: i32 = 10;
    println!("{}", x + y);
    println!("{}", x - y);
    println!("{}", x * y);
    println!("{}", x / y);
    {
        let mut x = 5;
        x+=1;
        println!("{}", x);
    };
    println!("Hello, world!");
}
